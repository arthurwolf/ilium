//! Where the Images scene gets its pictures: the list of entries (picked files and folders, or
//! one built-in URL), the folder discovery that builds it, and the [`ImageIo`] seam over the
//! host imports (`fs` on the read-only `picked/<n>` scopes, `http`, `codec`) so everything
//! above it is plain code a test can drive with a fake.
//!
//! Nothing here waits. Discovery lists a few folders per call and the scene calls it once per
//! frame until it reports `Done`.

use super::picture::RgbaImage;
use ilium_plugin_sdk::host::{EntryKind, HostCodec, HostFs, HostHttp, HttpRequest};
use ilium_plugin_sdk::types::paths::FsPath;
use ilium_plugin_sdk::types::RequestId;
use std::collections::VecDeque;

/// File extensions the scene shows (matched case-insensitively).
pub const IMAGE_EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "gif", "bmp", "webp"];
/// Upper bound on the entries one discovery keeps.
pub const MAX_FILES: usize = 20_000;
/// Deepest folder level descended into (the picked folder is level 0).
const MAX_DEPTH: usize = 12;
/// Folders listed at most by one discovery.
const MAX_DIRECTORIES: usize = 2_000;
/// Folders listed by one `Discovery::step`.
const DIRECTORIES_PER_STEP: usize = 8;
/// The host lists at most this many entries of one folder (`fs.list-dir` cap).
pub const LIST_DIR_ENTRIES: u32 = 4_096;
/// Longest note kept, in characters.
const MAX_NOTES: usize = 3;
/// A fetched picture is read from the host at most this large (the `http` body cap).
pub const MAX_ENCODED_BYTES: usize = 8 * 1024 * 1024;
/// A picture is refused when it has more pixels than this (RGBA8 is four bytes of each).
pub const MAX_SOURCE_PIXELS: u32 = 16_000_000;
/// Timeout asked of one download.
pub const FETCH_TIMEOUT_MILLIS: u32 = 30_000;

/// True when `name` ends with a supported image extension.
pub fn is_image_name(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(stem, extension)| {
        !stem.is_empty()
            && IMAGE_EXTENSIONS
                .iter()
                .any(|known| known.eq_ignore_ascii_case(extension))
    })
}

/// Where one entry's bytes come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntrySource {
    /// A file of the lent scope `picked/<scope>`; `path` is relative to the lent root and empty
    /// for a lent file.
    File { scope: usize, path: String },
    /// An https URL on a granted host.
    Url(String),
}

/// One picture of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Shown in the status line.
    pub name: String,
    pub source: EntrySource,
}

impl Entry {
    /// A stable text identity, used to seed the per-picture motion.
    pub fn key(&self) -> String {
        match &self.source {
            EntrySource::File { scope, path } => format!("picked/{scope}/{path}"),
            EntrySource::Url(url) => url.clone(),
        }
    }
}

/// What the scene knows about one folder item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirItem {
    pub name: String,
    pub kind: ItemKind,
}

/// The kinds of folder item discovery cares about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    File,
    Directory,
    /// A link or special file: never followed.
    Other,
}

/// One finished download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fetched {
    Body(Vec<u8>),
    /// A non-success HTTP status.
    Status(u16),
}

/// The host calls the scene makes. Implemented by [`HostIo`] in the component and by fakes in
/// tests.
pub trait ImageIo: Send {
    /// The items of folder `path` (relative, empty for the lent root) of scope `picked/<scope>`.
    fn list_dir(&mut self, scope: usize, path: &str) -> Result<Vec<DirItem>, String>;
    /// The whole file `path` of scope `picked/<scope>`.
    fn read_file(&mut self, scope: usize, path: &str) -> Result<Vec<u8>, String>;
    /// Starts a download; the id is polled with [`ImageIo::take_fetched`].
    fn fetch(&mut self, url: &str) -> Result<u64, String>;
    /// The finished answer of download `id`, or `None` while it runs.
    fn take_fetched(&mut self, id: u64) -> Option<Fetched>;
    /// Decodes a picture (any supported format) of at most [`MAX_SOURCE_PIXELS`] pixels.
    fn decode(&mut self, bytes: &[u8]) -> Result<RgbaImage, String>;
}

/// The host's `fs`, `http` and `codec` imports.
#[derive(Debug, Default, Clone, Copy)]
pub struct HostIo;

fn picked_path(scope: usize, path: &str) -> FsPath {
    FsPath {
        scope: format!("picked/{scope}"),
        path: path.to_owned(),
    }
}

impl ImageIo for HostIo {
    fn list_dir(&mut self, scope: usize, path: &str) -> Result<Vec<DirItem>, String> {
        let entries = HostFs::list_dir(
            &mut *ilium_plugin_sdk::host(),
            picked_path(scope, path),
            LIST_DIR_ENTRIES,
        )
        .map_err(|error| error.message)?;
        Ok(entries
            .into_iter()
            .map(|entry| DirItem {
                name: entry.name,
                kind: match entry.metadata.kind {
                    EntryKind::File => ItemKind::File,
                    EntryKind::Directory => ItemKind::Directory,
                    EntryKind::Symlink | EntryKind::Other => ItemKind::Other,
                },
            })
            .collect())
    }

    fn read_file(&mut self, scope: usize, path: &str) -> Result<Vec<u8>, String> {
        let mut host = ilium_plugin_sdk::host();
        let RequestId(id) = HostFs::read_all(&mut *host, picked_path(scope, path))
            .map_err(|error| error.message)?;
        HostFs::take_bytes(&mut *host, RequestId(id))
            .ok_or_else(|| "the file could not be read".to_owned())
    }

    fn fetch(&mut self, url: &str) -> Result<u64, String> {
        HostHttp::fetch(
            &mut *ilium_plugin_sdk::host(),
            HttpRequest {
                method: "GET".into(),
                url: url.into(),
                headers: vec![ilium_plugin_sdk::types::net::Header {
                    name: "Accept".into(),
                    value: "image/*".into(),
                }],
                body: None,
                timeout_millis: FETCH_TIMEOUT_MILLIS,
            },
        )
        .map(|RequestId(id)| id)
        .map_err(|error| error.message)
    }

    fn take_fetched(&mut self, id: u64) -> Option<Fetched> {
        let response = HostHttp::take_response(&mut *ilium_plugin_sdk::host(), RequestId(id))?;
        Some(if (200..300).contains(&response.status) {
            Fetched::Body(response.body)
        } else {
            Fetched::Status(response.status)
        })
    }

    fn decode(&mut self, bytes: &[u8]) -> Result<RgbaImage, String> {
        let image = HostCodec::decode_image(
            &mut *ilium_plugin_sdk::host(),
            bytes.to_vec(),
            0,
            MAX_SOURCE_PIXELS,
        )
        .map_err(|error| error.message)?;
        Ok(RgbaImage {
            width: image.width,
            height: image.height,
            rgba: image.rgba,
        })
    }
}

/// How one `Discovery::step` ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    Working,
    Done,
}

/// The folder walk of the slideshow, advanced a few folders per call.
pub struct Discovery {
    queue: VecDeque<(usize, String, usize)>,
    recursive: bool,
    entries: Vec<Entry>,
    notes: Vec<String>,
    listed: usize,
    failed_roots: usize,
    roots: usize,
}

/// The finished list.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Discovered {
    pub entries: Vec<Entry>,
    /// Short remarks for the status line (a folder that could not be read, a cap that was hit).
    pub notes: Vec<String>,
    /// Set when no folder could be read at all.
    pub error: Option<String>,
}

impl Discovery {
    /// Walks each `(scope, label)` folder; `recursive` descends into sub-folders.
    pub fn new(roots: &[(usize, String)], recursive: bool) -> Self {
        Self {
            queue: roots
                .iter()
                .map(|(scope, _)| (*scope, String::new(), 0))
                .collect(),
            recursive,
            entries: Vec::new(),
            notes: Vec::new(),
            listed: 0,
            failed_roots: 0,
            roots: roots.len(),
        }
    }

    fn note(&mut self, text: String) {
        if self.notes.len() < MAX_NOTES && !self.notes.contains(&text) {
            self.notes.push(text);
        }
    }

    /// Lists up to a few folders. `Done` once the queue is empty or a cap was hit.
    pub fn step(&mut self, io: &mut dyn ImageIo) -> Progress {
        for _ in 0..DIRECTORIES_PER_STEP {
            let Some((scope, path, depth)) = self.queue.pop_front() else {
                return Progress::Done;
            };
            if self.listed >= MAX_DIRECTORIES {
                self.note("folder limit reached".to_owned());
                self.queue.clear();
                return Progress::Done;
            }
            self.listed += 1;
            let items = match io.list_dir(scope, &path) {
                Ok(items) => items,
                Err(message) => {
                    if depth == 0 {
                        self.failed_roots += 1;
                    }
                    self.note(format!("unreadable folder: {message}"));
                    continue;
                }
            };
            if items.len() >= LIST_DIR_ENTRIES as usize {
                self.note("a folder was cut at 4096 items".to_owned());
            }
            for item in items {
                let relative = if path.is_empty() {
                    item.name.clone()
                } else {
                    format!("{path}/{}", item.name)
                };
                match item.kind {
                    ItemKind::Directory => {
                        if self.recursive && depth < MAX_DEPTH {
                            self.queue.push_back((scope, relative, depth + 1));
                        }
                    }
                    ItemKind::File if is_image_name(&item.name) => {
                        if self.entries.len() >= MAX_FILES {
                            self.note("more than 20000 pictures: the rest are left out".to_owned());
                            self.queue.clear();
                            return Progress::Done;
                        }
                        self.entries.push(Entry {
                            name: relative.clone(),
                            source: EntrySource::File {
                                scope,
                                path: relative,
                            },
                        });
                    }
                    ItemKind::File | ItemKind::Other => {}
                }
            }
        }
        if self.queue.is_empty() {
            Progress::Done
        } else {
            Progress::Working
        }
    }

    /// The finished list, sorted by name.
    pub fn finish(mut self) -> Discovered {
        self.entries.sort_by(|left, right| {
            left.name
                .to_lowercase()
                .cmp(&right.name.to_lowercase())
                .then_with(|| left.name.cmp(&right.name))
        });
        let error = if self.roots == 0 {
            Some("Choose a folder of pictures".to_owned())
        } else if self.failed_roots == self.roots {
            Some(
                self.notes
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "The folder could not be read".to_owned()),
            )
        } else if self.entries.is_empty() {
            Some("No pictures found in the chosen folders".to_owned())
        } else {
            None
        };
        Discovered {
            entries: self.entries,
            notes: self.notes,
            error,
        }
    }
}

#[cfg(test)]
pub(super) mod fake {
    use super::*;
    use std::collections::HashMap;

    /// A scripted [`ImageIo`]: folders and files keyed by scope and path, URLs answered at once
    /// or after a number of polls.
    #[derive(Default)]
    pub struct FakeIo {
        /// `(scope, folder path)` to its items.
        pub folders: HashMap<(usize, String), Vec<DirItem>>,
        /// `(scope, file path)` to its bytes.
        pub files: HashMap<(usize, String), Vec<u8>>,
        /// URL to the answer it gets.
        pub urls: HashMap<String, Fetched>,
        /// Polls a download needs before it answers.
        polls_before_answer: usize,
        /// Every folder listed, in call order.
        pub list_calls: Vec<(usize, String)>,
        /// Pictures decoded; a picture's bytes are `[width, height, r, g, b]`.
        pub decode_calls: usize,
        running: HashMap<u64, (String, usize)>,
        next_id: u64,
    }

    impl FakeIo {
        /// A fake whose downloads need `polls` polls before they answer.
        pub fn answering_after(polls: usize) -> Self {
            Self {
                polls_before_answer: polls,
                ..Self::default()
            }
        }

        pub fn folder(&mut self, scope: usize, path: &str, items: &[(&str, ItemKind)]) {
            self.folders.insert(
                (scope, path.to_owned()),
                items
                    .iter()
                    .map(|(name, kind)| DirItem {
                        name: (*name).to_owned(),
                        kind: *kind,
                    })
                    .collect(),
            );
        }

        /// A picture "file": the bytes carry the size and colour [`FakeIo::decode`] returns.
        pub fn picture(width: u8, height: u8, color: [u8; 3]) -> Vec<u8> {
            vec![width, height, color[0], color[1], color[2]]
        }
    }

    impl ImageIo for FakeIo {
        fn list_dir(&mut self, scope: usize, path: &str) -> Result<Vec<DirItem>, String> {
            self.list_calls.push((scope, path.to_owned()));
            self.folders
                .get(&(scope, path.to_owned()))
                .cloned()
                .ok_or_else(|| "not found".to_owned())
        }

        fn read_file(&mut self, scope: usize, path: &str) -> Result<Vec<u8>, String> {
            self.files
                .get(&(scope, path.to_owned()))
                .cloned()
                .ok_or_else(|| "not found".to_owned())
        }

        fn fetch(&mut self, url: &str) -> Result<u64, String> {
            if !self.urls.contains_key(url) {
                return Err("host not granted".to_owned());
            }
            self.next_id += 1;
            self.running
                .insert(self.next_id, (url.to_owned(), self.polls_before_answer));
            Ok(self.next_id)
        }

        fn take_fetched(&mut self, id: u64) -> Option<Fetched> {
            let (url, waits) = self.running.get_mut(&id)?;
            if *waits > 0 {
                *waits -= 1;
                return None;
            }
            let url = url.clone();
            self.running.remove(&id);
            self.urls.get(&url).cloned()
        }

        fn decode(&mut self, bytes: &[u8]) -> Result<RgbaImage, String> {
            self.decode_calls += 1;
            let [width, height, red, green, blue] = *bytes else {
                return Err("not a picture".to_owned());
            };
            let (width, height) = (u32::from(width), u32::from(height));
            let mut rgba = Vec::with_capacity((width * height * 4) as usize);
            for _ in 0..width * height {
                rgba.extend_from_slice(&[red, green, blue, 255]);
            }
            Ok(RgbaImage {
                width,
                height,
                rgba,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::FakeIo;
    use super::*;

    fn run(discovery: Discovery, io: &mut FakeIo) -> Discovered {
        let mut discovery = discovery;
        for _ in 0..1_000 {
            if discovery.step(io) == Progress::Done {
                return discovery.finish();
            }
        }
        panic!("discovery must finish");
    }

    #[test]
    fn extensions_match_any_case_and_need_a_stem() {
        assert!(is_image_name("a.png"));
        assert!(is_image_name("A.JPG"));
        assert!(is_image_name("deep.name.webp"));
        assert!(!is_image_name("notes.txt"));
        assert!(!is_image_name("png"));
        assert!(!is_image_name(".png"));
        assert!(!is_image_name("a.pngx"));
    }

    #[test]
    fn a_folder_walk_finds_pictures_in_name_order_and_descends_when_asked() {
        let mut io = FakeIo::default();
        io.folder(
            1,
            "",
            &[
                ("b.png", ItemKind::File),
                ("A.jpg", ItemKind::File),
                ("readme.txt", ItemKind::File),
                ("sub", ItemKind::Directory),
                ("link", ItemKind::Other),
            ],
        );
        io.folder(
            1,
            "sub",
            &[("c.gif", ItemKind::File), ("d", ItemKind::Directory)],
        );
        io.folder(1, "sub/d", &[("e.bmp", ItemKind::File)]);

        let deep = run(Discovery::new(&[(1, "pics".to_owned())], true), &mut io);
        let names: Vec<&str> = deep
            .entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect();
        assert_eq!(names, ["A.jpg", "b.png", "sub/c.gif", "sub/d/e.bmp"]);
        assert_eq!(deep.error, None);
        assert_eq!(
            deep.entries[2].source,
            EntrySource::File {
                scope: 1,
                path: "sub/c.gif".to_owned()
            }
        );

        let shallow = run(Discovery::new(&[(1, "pics".to_owned())], false), &mut io);
        assert_eq!(shallow.entries.len(), 2);
    }

    #[test]
    fn several_folders_are_walked_and_an_unreadable_one_is_only_a_note() {
        let mut io = FakeIo::default();
        io.folder(1, "", &[("one.png", ItemKind::File)]);
        let found = run(
            Discovery::new(&[(1, "a".to_owned()), (2, "b".to_owned())], true),
            &mut io,
        );
        assert_eq!(found.entries.len(), 1);
        assert_eq!(found.error, None);
        assert_eq!(found.notes.len(), 1);
        assert!(found.notes[0].starts_with("unreadable folder"));
    }

    #[test]
    fn no_folder_readable_or_no_picture_is_an_error() {
        let mut io = FakeIo::default();
        let none = run(Discovery::new(&[], true), &mut io);
        assert_eq!(none.error.as_deref(), Some("Choose a folder of pictures"));

        let unreadable = run(Discovery::new(&[(1, "x".to_owned())], true), &mut io);
        assert!(unreadable.error.is_some());

        io.folder(1, "", &[("notes.txt", ItemKind::File)]);
        let empty = run(Discovery::new(&[(1, "x".to_owned())], true), &mut io);
        assert_eq!(
            empty.error.as_deref(),
            Some("No pictures found in the chosen folders")
        );
    }

    #[test]
    fn a_step_lists_a_bounded_number_of_folders() {
        let mut io = FakeIo::default();
        let mut items = Vec::new();
        let names: Vec<String> = (0..20).map(|number| format!("d{number}")).collect();
        for name in &names {
            items.push((name.as_str(), ItemKind::Directory));
            io.folder(1, name, &[("p.png", ItemKind::File)]);
        }
        io.folder(1, "", &items);
        let mut discovery = Discovery::new(&[(1, "x".to_owned())], true);
        assert_eq!(discovery.step(&mut io), Progress::Working);
        assert_eq!(io.list_calls.len(), DIRECTORIES_PER_STEP);
        while discovery.step(&mut io) == Progress::Working {}
        assert_eq!(discovery.finish().entries.len(), 20);
    }

    #[test]
    fn entry_keys_are_stable_and_distinct() {
        let file = Entry {
            name: "a.png".into(),
            source: EntrySource::File {
                scope: 2,
                path: "a.png".into(),
            },
        };
        let url = Entry {
            name: "u".into(),
            source: EntrySource::Url("https://x.test/a.png".into()),
        };
        assert_eq!(file.key(), "picked/2/a.png");
        assert_eq!(url.key(), "https://x.test/a.png");
        assert_ne!(file.key(), url.key());
    }
}
