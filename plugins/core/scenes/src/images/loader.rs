//! Loading one picture at a time, a slice of work per frame: read the file or start the
//! download, wait for the answer, hand the bytes to the host decoder, then box-filter the result
//! down to the decode target in bounded slices. Nothing here waits or spawns; the scene calls
//! [`Loader::step`] once per frame and receives the finished picture (or why it failed) as an
//! [`Event`].

use super::picture::{Downscale, Picture};
use super::source::{
    Entry, EntrySource, Fetched, ImageIo, FETCH_TIMEOUT_MILLIS, MAX_ENCODED_BYTES,
};
use std::sync::Arc;
use std::time::Duration;

/// Source pixels one frame may spend on the downscale (a few milliseconds of work).
pub const SLICE_PIXELS: usize = 600_000;
/// The host reports a transport failure as a download that never answers, so a download is
/// given up on after its own timeout plus this margin.
const DEADLINE_MARGIN: Duration = Duration::from_secs(7);

/// What the scene asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadRequest {
    /// Index into the scene's entry list.
    pub index: usize,
    /// The scene's generation; an answer for an older one is dropped by the scene.
    pub generation: u32,
    pub entry: Entry,
    /// The picture is reduced to fit inside this many pixels.
    pub max_width: u32,
    pub max_height: u32,
}

/// What the loader reports.
#[derive(Debug)]
pub enum Event {
    Loaded {
        index: usize,
        generation: u32,
        image: Arc<Picture>,
    },
    Failed {
        index: usize,
        generation: u32,
        message: String,
    },
}

enum Work {
    /// Nothing done yet: read the file or start the download.
    Start(Entry),
    Fetching {
        id: u64,
        deadline: Duration,
    },
    Decoding(Vec<u8>),
    Scaling(Downscale),
}

struct Job {
    index: usize,
    generation: u32,
    max: (u32, u32),
    work: Work,
}

/// The one-picture-at-a-time loader.
#[derive(Default)]
pub struct Loader {
    job: Option<Job>,
}

impl Loader {
    /// True while a picture is being loaded.
    pub fn is_busy(&self) -> bool {
        self.job.is_some()
    }

    /// Starts loading `request`; `false` (and nothing changes) while another load is running.
    pub fn request(&mut self, request: LoadRequest) -> bool {
        if self.job.is_some() {
            return false;
        }
        self.job = Some(Job {
            index: request.index,
            generation: request.generation,
            max: (request.max_width, request.max_height),
            work: Work::Start(request.entry),
        });
        true
    }

    /// Forgets the running load (the generation changed).
    pub fn cancel(&mut self) {
        self.job = None;
    }

    /// Advances the running load by one stage; `now` is the scene's wall clock.
    pub fn step(&mut self, io: &mut dyn ImageIo, now: Duration) -> Option<Event> {
        let job = self.job.as_mut()?;
        let (index, generation) = (job.index, job.generation);
        let outcome = Self::advance(job, io, now);
        match outcome {
            Ok(None) => None,
            Ok(Some(image)) => {
                self.job = None;
                Some(Event::Loaded {
                    index,
                    generation,
                    image,
                })
            }
            Err(message) => {
                self.job = None;
                Some(Event::Failed {
                    index,
                    generation,
                    message,
                })
            }
        }
    }

    fn advance(
        job: &mut Job,
        io: &mut dyn ImageIo,
        now: Duration,
    ) -> Result<Option<Arc<Picture>>, String> {
        let work = std::mem::replace(&mut job.work, Work::Decoding(Vec::new()));
        match work {
            Work::Start(entry) => {
                job.work = match &entry.source {
                    EntrySource::File { scope, path } => {
                        let bytes = io
                            .read_file(*scope, path)
                            .map_err(|message| format!("{}: {message}", entry.name))?;
                        Self::checked(bytes, &entry.name).map(Work::Decoding)?
                    }
                    EntrySource::Url(url) => {
                        let id = io
                            .fetch(url)
                            .map_err(|message| format!("{}: {message}", entry.name))?;
                        Work::Fetching {
                            id,
                            deadline: now
                                + Duration::from_millis(u64::from(FETCH_TIMEOUT_MILLIS))
                                + DEADLINE_MARGIN,
                        }
                    }
                };
                Ok(None)
            }
            Work::Fetching { id, deadline } => match io.take_fetched(id) {
                None if now >= deadline => Err("the download timed out".to_owned()),
                None => {
                    job.work = Work::Fetching { id, deadline };
                    Ok(None)
                }
                Some(Fetched::Status(status)) => Err(format!("the server answered {status}")),
                Some(Fetched::Body(bytes)) => {
                    job.work = Self::checked(bytes, "download").map(Work::Decoding)?;
                    Ok(None)
                }
            },
            Work::Decoding(bytes) => {
                let decoded = io.decode(&bytes)?;
                drop(bytes);
                let scaling = Downscale::new(decoded, job.max.0, job.max.1)
                    .ok_or_else(|| "the picture is damaged".to_owned())?;
                job.work = Work::Scaling(scaling);
                Ok(None)
            }
            Work::Scaling(mut scaling) => match scaling.step(SLICE_PIXELS) {
                Some(picture) => Ok(Some(Arc::new(picture))),
                None => {
                    job.work = Work::Scaling(scaling);
                    Ok(None)
                }
            },
        }
    }

    fn checked(bytes: Vec<u8>, name: &str) -> Result<Vec<u8>, String> {
        if bytes.is_empty() {
            Err(format!("{name}: the file is empty"))
        } else if bytes.len() > MAX_ENCODED_BYTES {
            Err(format!("{name}: larger than 8 MiB"))
        } else {
            Ok(bytes)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::source::fake::FakeIo;
    use super::*;

    fn file_entry(name: &str) -> Entry {
        Entry {
            name: name.to_owned(),
            source: EntrySource::File {
                scope: 0,
                path: String::new(),
            },
        }
    }

    fn request(entry: Entry) -> LoadRequest {
        LoadRequest {
            index: 3,
            generation: 7,
            entry,
            max_width: 8,
            max_height: 8,
        }
    }

    fn run(loader: &mut Loader, io: &mut FakeIo, clock: &mut Duration) -> Event {
        for _ in 0..1_000 {
            *clock += Duration::from_millis(80);
            if let Some(event) = loader.step(io, *clock) {
                return event;
            }
        }
        panic!("the load must finish");
    }

    #[test]
    fn a_file_is_read_decoded_and_reduced_over_several_frames() {
        let mut io = FakeIo::default();
        io.files
            .insert((0, String::new()), FakeIo::picture(32, 16, [10, 20, 30]));
        let mut loader = Loader::default();
        assert!(loader.request(request(file_entry("a.png"))));
        assert!(loader.is_busy());
        let mut clock = Duration::ZERO;
        let Event::Loaded {
            index,
            generation,
            image,
        } = run(&mut loader, &mut io, &mut clock)
        else {
            panic!("expected a picture");
        };
        assert_eq!((index, generation), (3, 7));
        assert_eq!((image.width, image.height), (8, 4));
        assert!(image.pixels.iter().all(|pixel| *pixel == [10, 20, 30]));
        assert!(!loader.is_busy());
        assert_eq!(io.decode_calls, 1);
    }

    #[test]
    fn only_one_load_runs_at_a_time() {
        let mut loader = Loader::default();
        assert!(loader.request(request(file_entry("a.png"))));
        assert!(!loader.request(request(file_entry("b.png"))));
        loader.cancel();
        assert!(!loader.is_busy());
        assert!(loader.request(request(file_entry("b.png"))));
    }

    #[test]
    fn a_download_waits_for_its_answer_and_then_loads() {
        let url = "https://images.test/a.jpg";
        let mut io = FakeIo::answering_after(3);
        io.urls.insert(
            url.to_owned(),
            Fetched::Body(FakeIo::picture(4, 4, [1, 2, 3])),
        );
        let mut loader = Loader::default();
        loader.request(request(Entry {
            name: "a".into(),
            source: EntrySource::Url(url.into()),
        }));
        let mut clock = Duration::ZERO;
        let event = run(&mut loader, &mut io, &mut clock);
        assert!(matches!(event, Event::Loaded { .. }));
    }

    #[test]
    fn a_download_that_never_answers_times_out() {
        let url = "https://images.test/slow.jpg";
        let mut io = FakeIo::answering_after(usize::MAX);
        io.urls.insert(url.to_owned(), Fetched::Status(200));
        let mut loader = Loader::default();
        loader.request(request(Entry {
            name: "slow".into(),
            source: EntrySource::Url(url.into()),
        }));
        let mut clock = Duration::ZERO;
        let Event::Failed { message, .. } = run(&mut loader, &mut io, &mut clock) else {
            panic!("expected a failure");
        };
        assert_eq!(message, "the download timed out");
        assert!(clock >= Duration::from_secs(37));
    }

    #[test]
    fn failures_are_reported_with_the_reason() {
        let mut loader = Loader::default();
        let mut io = FakeIo::default();
        let mut clock = Duration::ZERO;

        // A file the host cannot read.
        loader.request(request(file_entry("gone.png")));
        let Event::Failed { message, .. } = run(&mut loader, &mut io, &mut clock) else {
            panic!("expected a failure");
        };
        assert!(message.starts_with("gone.png"));

        // A file that is not a picture.
        io.files.insert((0, String::new()), vec![1, 2, 3]);
        loader.request(request(file_entry("bad.png")));
        let Event::Failed { message, .. } = run(&mut loader, &mut io, &mut clock) else {
            panic!("expected a failure");
        };
        assert_eq!(message, "not a picture");

        // An empty file and an oversized one.
        io.files.insert((0, String::new()), Vec::new());
        loader.request(request(file_entry("empty.png")));
        assert!(matches!(
            run(&mut loader, &mut io, &mut clock),
            Event::Failed { .. }
        ));
        io.files
            .insert((0, String::new()), vec![0; MAX_ENCODED_BYTES + 1]);
        loader.request(request(file_entry("huge.png")));
        let Event::Failed { message, .. } = run(&mut loader, &mut io, &mut clock) else {
            panic!("expected a failure");
        };
        assert!(message.contains("8 MiB"));

        // A server error and a host that refuses the address.
        io.urls
            .insert("https://images.test/missing".into(), Fetched::Status(404));
        loader.request(request(Entry {
            name: "missing".into(),
            source: EntrySource::Url("https://images.test/missing".into()),
        }));
        let Event::Failed { message, .. } = run(&mut loader, &mut io, &mut clock) else {
            panic!("expected a failure");
        };
        assert_eq!(message, "the server answered 404");
        loader.request(request(Entry {
            name: "blocked".into(),
            source: EntrySource::Url("https://other.test/x".into()),
        }));
        assert!(matches!(
            run(&mut loader, &mut io, &mut clock),
            Event::Failed { .. }
        ));
    }

    #[test]
    fn an_idle_loader_does_nothing() {
        let mut loader = Loader::default();
        let mut io = FakeIo::default();
        assert!(loader.step(&mut io, Duration::ZERO).is_none());
    }
}
