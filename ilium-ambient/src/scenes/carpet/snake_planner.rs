//! Bounded shortest food routes inside the snake's free forward cycle arc.
//!
//! The body keeps its cyclic order, but need not occupy consecutive cycle
//! cells. A route advances that order at every move and stays before the
//! current tail. Moving the tail can only release additional room, so the
//! entire committed route remains safe, including growth at its destination.
use std::collections::VecDeque;

pub(super) fn food_route(
    side: usize,
    cycle: &[[usize; 2]],
    ranks: &[usize],
    head: usize,
    tail: usize,
    food: &[usize],
) -> VecDeque<usize> {
    let size = cycle.len();
    let tail_distance = (tail + size - head) % size;
    let distance = |rank: usize| (rank + size - head) % size;
    let mut parent = vec![size; size];
    let mut queue = VecDeque::with_capacity(size);
    parent[head] = head;
    queue.push_back(head);
    while let Some(current) = queue.pop_front() {
        let [x, y] = cycle[current];
        let neighbors = [
            (x + 1 < side).then_some([x + 1, y]),
            (y + 1 < side).then_some([x, y + 1]),
            x.checked_sub(1).map(|x| [x, y]),
            y.checked_sub(1).map(|y| [x, y]),
        ];
        for [x, y] in neighbors.into_iter().flatten() {
            let next = ranks[y * side + x];
            let next_distance = distance(next);
            if parent[next] != size
                || next_distance <= distance(current)
                || next_distance >= tail_distance
            {
                continue;
            }
            parent[next] = current;
            if food.contains(&next) {
                let mut route = VecDeque::new();
                let mut cursor = next;
                while cursor != head {
                    route.push_front(cursor);
                    cursor = parent[cursor];
                }
                return route;
            }
            queue.push_back(next);
        }
    }
    // Each rank enters the queue at most once: at most N expanded cells,
    // four neighbor checks per cell, and N retained parent/queue entries.
    VecDeque::new()
}
