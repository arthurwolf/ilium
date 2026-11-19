//! Bounded framing of Smart Copy's correlated candidate response.

const CHUNK_BYTES: usize = 128 * 1024;

struct ResponseFrames {
    response: String,
    boundaries: Vec<usize>,
    instance: u64,
    sequence: u64,
}

impl ResponseFrames {
    fn new(response: String, instance: u64, sequence: u64) -> Self {
        let mut boundaries = vec![0];
        let mut start: usize = 0;
        while start < response.len() {
            let mut end = start.saturating_add(CHUNK_BYTES).min(response.len());
            while !response.is_char_boundary(end) {
                end -= 1;
            }
            boundaries.push(end);
            start = end;
        }
        if boundaries.len() == 1 {
            boundaries.push(0);
        }
        Self { response, boundaries, instance, sequence }
    }

    fn len(&self) -> usize { self.boundaries.len() - 1 }

    fn retained_bytes(&self) -> Option<usize> {
        self.boundaries.capacity().checked_mul(std::mem::size_of::<usize>())?
            .checked_add(self.response.capacity())
    }

    fn frame(&self, index: usize) -> Option<Vec<u8>> {
        let start = *self.boundaries.get(index)?;
        let end = *self.boundaries.get(index.checked_add(1)?)?;
        let chunk = &self.response[start..end];
        serde_json::to_vec(&serde_json::json!({
            "instance": self.instance,
            "sequence": self.sequence,
            "chunk_index": index,
            "chunk_count": self.len(),
            "chunk": chunk,
        })).ok()
    }
}

struct Transfer {
    frames: ResponseFrames,
    touched: u64,
}

struct TransferBook {
    entries: std::collections::HashMap<u64, Transfer>,
    max_entries: usize,
    max_bytes: usize,
    ttl_millis: u64,
}

impl TransferBook {
    fn new(max_entries: usize, max_bytes: usize, ttl_millis: u64) -> Self {
        Self { entries: std::collections::HashMap::new(), max_entries, max_bytes, ttl_millis }
    }

    fn insert(&mut self, client: u64, instance: u64, sequence: u64, response: String, now: u64) -> Result<(), ()> {
        self.expire(now);
        if !self.entries.contains_key(&client) && self.entries.len() >= self.max_entries {
            return Err(());
        }
        let frames = ResponseFrames::new(response, instance, sequence);
        let used = self.entries.iter().filter(|(owner, _)| **owner != client)
            .try_fold(0usize, |total, (_, transfer)| total.checked_add(transfer.frames.retained_bytes()?))
            .ok_or(())?;
        if used.checked_add(frames.retained_bytes().ok_or(())?).is_none_or(|total| total > self.max_bytes) {
            return Err(());
        }
        self.entries.insert(client, Transfer { frames, touched: now });
        Ok(())
    }

    fn frame(&mut self, client: u64, instance: u64, sequence: u64, index: usize, now: u64) -> Option<Vec<u8>> {
        let entry = self.entries.get_mut(&client)?;
        if now.saturating_sub(entry.touched) >= self.ttl_millis
            || entry.frames.instance != instance || entry.frames.sequence != sequence {
            return None;
        }
        let frame = entry.frames.frame(index)?;
        entry.touched = now;
        Some(frame)
    }

    fn expire(&mut self, now: u64) {
        self.entries.retain(|_, entry| now.saturating_sub(entry.touched) < self.ttl_millis);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaped_unicode_frames_reassemble_without_exceeding_the_message_limit() {
        let response = format!("{}{}{}", "\0".repeat(140_000), "\\\"".repeat(500_000), "🙂".repeat(100_000));
        let frames = ResponseFrames::new(response.clone(), 7, 11);
        assert!(frames.len() > 1);
        let mut reconstructed = String::new();
        for index in 0..frames.len() {
            let frame = frames.frame(index).expect("frame");
            assert!(frame.len() <= 1024 * 1024);
            let value: serde_json::Value = serde_json::from_slice(&frame).unwrap();
            assert_eq!(value["instance"].as_u64(), Some(7));
            assert_eq!(value["sequence"].as_u64(), Some(11));
            assert_eq!(value["chunk_index"].as_u64(), Some(index as u64));
            assert_eq!(value["chunk_count"].as_u64(), Some(frames.len() as u64));
            reconstructed.push_str(value["chunk"].as_str().unwrap());
        }
        assert_eq!(reconstructed, response);
        assert!(frames.frame(usize::MAX).is_none());
    }

    #[test]
    fn empty_response_has_one_valid_frame() {
        let frames = ResponseFrames::new(String::new(), 1, 2);
        assert_eq!(frames.len(), 1);
        let frame = frames.frame(0).expect("empty response frame");
        let value: serde_json::Value = serde_json::from_slice(&frame).unwrap();
        assert_eq!(value["chunk"].as_str(), Some(""));
        assert!(frames.frame(1).is_none());
    }

    #[test]
    fn transfers_are_client_scoped_retryable_and_expire() {
        let mut book = TransferBook::new(2, 1024, 5000);
        book.insert(3, 7, 9, "private".to_owned(), 0).expect("admit");
        let first = book.frame(3, 7, 9, 0, 1).expect("first frame");
        assert_eq!(book.frame(3, 7, 9, 0, 2), Some(first));
        assert!(book.frame(4, 7, 9, 0, 2).is_none());
        assert!(book.frame(3, 7, 10, 0, 2).is_none());
        book.expire(5002);
        assert!(book.frame(3, 7, 9, 0, 5002).is_none());
    }

    #[test]
    fn admission_refusal_preserves_existing_transfers() {
        let mut book = TransferBook::new(1, 64, 5000);
        book.insert(1, 1, 1, "existing".to_owned(), 0).expect("admit");
        assert!(book.insert(2, 1, 1, "other".to_owned(), 0).is_err());
        assert!(book.frame(1, 1, 1, 0, 0).is_some());
        assert!(book.insert(1, 1, 2, "x".repeat(65), 0).is_err());
        assert!(book.frame(1, 1, 1, 0, 0).is_some());
    }

    #[test]
    fn admission_accounts_for_frame_offsets_as_well_as_response() {
        let mut book = TransferBook::new(1, 1, 5000);
        assert!(book.insert(1, 1, 1, "x".to_owned(), 0).is_err());
    }
}
