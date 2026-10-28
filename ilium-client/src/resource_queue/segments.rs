//! Bounded, deterministic queue-strip geometry. Estimates never grant ownership.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueSegment {
    pub entry: usize,
    pub cells: u16,
    pub overflow: usize,
}
/// Keep the selected waiter visible even when the queue exceeds the cell budget.
/// Every visible entry has at least one cell; an overflow segment represents omitted entries.
pub fn layout_queue_segments(
    weights: &[u64],
    selected: Option<usize>,
    width: u16,
) -> Vec<QueueSegment> {
    if width == 0 || weights.is_empty() {
        return Vec::new();
    }
    let count = weights.len();
    if count > usize::from(width) {
        if width == 1 {
            return vec![QueueSegment {
                entry: selected.filter(|i| *i < count).unwrap_or(0),
                cells: 1,
                overflow: count - 1,
            }];
        }
        let mut entries: Vec<usize> = (0..usize::from(width) - 1).collect();
        if let Some(index) = selected.filter(|i| *i < count) {
            if !entries.contains(&index) {
                let last = entries.len() - 1;
                entries[last] = index;
                entries.sort_unstable();
            }
        }
        let mut output: Vec<_> = entries
            .iter()
            .map(|index| QueueSegment {
                entry: *index,
                cells: 1,
                overflow: 0,
            })
            .collect();
        output.push(QueueSegment {
            entry: count,
            cells: 1,
            overflow: count - entries.len(),
        });
        return output;
    }
    let remaining = usize::from(width) - count;
    let total: u128 = weights
        .iter()
        .map(|weight| u128::from((*weight).max(1)))
        .sum();
    let mut output: Vec<_> = weights
        .iter()
        .enumerate()
        .map(|(entry, weight)| QueueSegment {
            entry,
            cells: 1 + ((u128::from((*weight).max(1)) * remaining as u128) / total) as u16,
            overflow: 0,
        })
        .collect();
    let used: u16 = output.iter().map(|entry| entry.cells).sum();
    let mut remainders: Vec<_> = weights
        .iter()
        .enumerate()
        .map(|(index, weight)| {
            (
                index,
                u128::from((*weight).max(1)) * remaining as u128 % total,
            )
        })
        .collect();
    remainders.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    for (index, _) in remainders.into_iter().take(usize::from(width - used)) {
        output[index].cells += 1;
    }
    output
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn allocation_fills_budget_and_keeps_short_participants() {
        let result = layout_queue_segments(&[600, 300, 60], Some(2), 80);
        assert_eq!(result.iter().map(|s| s.cells).sum::<u16>(), 80);
        assert!(result.iter().all(|s| s.cells > 0));
        assert!(result[0].cells > result[1].cells);
    }
    #[test]
    fn overflowing_queue_keeps_selected_and_reports_omissions() {
        let result = layout_queue_segments(&[1; 20], Some(17), 4);
        assert!(result.iter().any(|s| s.entry == 17));
        assert_eq!(result.iter().map(|s| s.cells).sum::<u16>(), 4);
        assert_eq!(result.last().unwrap().overflow, 17);
    }
    #[test]
    fn zero_and_extreme_weights_are_bounded() {
        for width in 0..120 {
            let result = layout_queue_segments(&[0, u64::MAX, 1], Some(2), width);
            assert_eq!(result.iter().map(|s| s.cells).sum::<u16>(), width);
        }
    }
}
