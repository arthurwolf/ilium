//! Conservative hierarchy for the unchanged analytic capsule maximum. // No contributor shortlist.
use super::super::model::Prepared; // Reuse the exact prepared primitive arithmetic.
#[derive(Debug, Default)] // Keep construction allocation-free until the first real population.
pub(super) struct EnvelopeTreeIndex {
    // This tree belongs to one persistent renderer.
    nodes: Vec<EnvelopeNode>, // Preorder nodes permit stack-free queries.
    order: Vec<usize>,        // Leaves refer to the renderer's canonical body indices.
} // End the hierarchy storage.
#[derive(Debug, Clone, Copy, Default)] // Nodes contain no heap allocations.
struct EnvelopeNode {
    // Bound the union of center segments before adding the maximum radius.
    axis: [f32; 2],      // One rectangle axis; the other is its perpendicular.
    low: [f32; 2],       // Outward-rounded projection minima of every contained endpoint.
    high: [f32; 2],      // Outward-rounded projection maxima of every contained endpoint.
    radius: f32,         // Maximum contained support radius, expanded for rounding.
    inverse_radius: f32, // Reuse the expanded squared-radius reciprocal at every query.
    height: f32,         // Maximum contained peak height.
    begin: usize,        // First body index in this node's leaf range.
    count: usize,        // Zero means an internal node.
    end: usize,          // First node after this entire subtree.
} // End the node record.
impl EnvelopeNode {
    // Node bounds only prune; actual values always use Prepared::sample.
    fn upper(&self, point: [f32; 2], softness: f32) -> f32 {
        // Bound all contributors below this node.
        let projected = [
            point[0] * self.axis[0] + point[1] * self.axis[1],
            -point[0] * self.axis[1] + point[1] * self.axis[0],
        ]; // Rotate without changing the query point.
        let dx = (self.low[0] - projected[0])
            .max(projected[0] - self.high[0])
            .max(0.0); // Distance outside the first interval.
        let dy = (self.low[1] - projected[1])
            .max(projected[1] - self.high[1])
            .max(0.0); // Distance outside the second interval.
        let distance_squared = (dx * dx + dy * dy) * 0.99999; // Round the nearly orthonormal squared distance downward.
        let u = (1.0 - distance_squared * self.inverse_radius).clamp(0.0, 1.0); // The maximum radius gives an upper cap.
        if u == 0.0 {
            return 0.0;
        } // The expanded support cannot reach this point.
        self.height * u * u * (1.0 - softness + softness * u) + 0.000004 // Round the height bound outward, never the actual sample.
    } // End the upper-bound query.
} // End node methods.
impl EnvelopeTreeIndex {
    // Construction is bounded by the accepted 4096-body population.
    #[cfg(test)] // Capacity probes do not enlarge the production API.
    pub(super) fn storage(&self) -> (usize, usize) {
        (self.nodes.capacity(), self.order.capacity())
    } // Expose retained capacities to regression tests and measurements.
    pub(super) fn build(
        &mut self,
        bodies: &[Prepared],
        stop: Option<&std::sync::atomic::AtomicBool>,
    ) -> bool {
        // Rebuild only when a general field repair needs the hierarchy.
        self.nodes.clear(); // Reuse the previous node allocation.
        self.order.clear(); // Reuse the previous permutation allocation.
        self.order.extend(0..bodies.len()); // Retain every canonical contributor.
        if bodies.is_empty() {
            return true;
        } // An empty field needs no root.
        self.build_node(bodies, 0, bodies.len(), stop) // Build a balanced preorder hierarchy.
    } // End hierarchy construction.
    fn build_node(
        &mut self,
        bodies: &[Prepared],
        begin: usize,
        count: usize,
        stop: Option<&std::sync::atomic::AtomicBool>,
    ) -> bool {
        // Recursion has at most ten levels at 4096 bodies.
        if super::stopping(stop) {
            return false;
        } // Never continue subdivision after owned cancellation.
        let node_index = self.nodes.len(); // Reserve the parent before either child.
        self.nodes.push(EnvelopeNode::default()); // Children remain contiguous in preorder.
        let representative = bodies[self.order[begin + count / 2]]; // Select an existing center segment for the rectangle orientation.
        let delta = [
            representative.key[2] - representative.key[0],
            representative.key[3] - representative.key[1],
        ]; // Recover its direction without altering the primitive.
        let length = f64::from(delta[0]).hypot(f64::from(delta[1])); // Avoid f32 subnormal normalization distorting the bounding axes.
        let axis = if length > 0.0 {
            delta.map(|v| (f64::from(v) / length) as f32)
        } else {
            [1.0, 0.0]
        }; // Spheres use the ground axes.
        let mut node = EnvelopeNode {
            axis,
            low: [f32::INFINITY; 2],
            high: [f32::NEG_INFINITY; 2],
            begin,
            count,
            ..Default::default()
        }; // Accumulate conservative node bounds.
        let mut low = [f32::INFINITY; 4]; // Measure endpoint-coordinate spreads for the spatial split.
        let mut high = [f32::NEG_INFINITY; 4]; // Keep all four endpoint dimensions.
        let mut lowest_height = f32::INFINITY; // Separate substantially different height populations first.
        for &index in &self.order[begin..begin + count] {
            // Visit every body assigned to this node.
            let body = bodies[index]; // Copy the small immutable primitive record.
            node.radius = node.radius.max(body.key[4]); // Larger support can only loosen the bound.
            node.height = node.height.max(body.height); // A lower body cannot exceed this peak.
            lowest_height = lowest_height.min(body.height); // Record the node's height spread.
            for coordinate in 0..4 {
                low[coordinate] = low[coordinate].min(body.key[coordinate]);
                high[coordinate] = high[coordinate].max(body.key[coordinate]);
            } // Accumulate the split extents.
            for endpoint in [[body.key[0], body.key[1]], [body.key[2], body.key[3]]] {
                // A rectangle containing all endpoints also contains every center segment.
                let projected = [
                    endpoint[0] * axis[0] + endpoint[1] * axis[1],
                    -endpoint[0] * axis[1] + endpoint[1] * axis[0],
                ]; // Use the same projection order as queries.
                for (coordinate, value) in projected.into_iter().enumerate() {
                    node.low[coordinate] = node.low[coordinate].min(value);
                    node.high[coordinate] = node.high[coordinate].max(value);
                } // Expand both rectangle intervals.
            } // Finish this body's endpoint bounds.
        } // Finish the node's population bounds.
        node.low = node.low.map(|v| v - 0.000004); // Cover endpoint, axis, query, and Prepared rounding at ground scale.
        node.high = node.high.map(|v| v + 0.000004); // Never contract the geometric enclosure.
        node.radius += 0.000004; // Include residual distance rounding in the support bound.
        node.inverse_radius = 1.0 / (node.radius * node.radius); // Prepare this bound operand once, not once per queried vertex.
        if count > 8 {
            // Small leaves avoid excessive hierarchy overhead.
            let mut coordinate = 0; // Resolve equal spatial spreads deterministically.
            for candidate in 1..4 {
                if high[candidate] - low[candidate] > high[coordinate] - low[coordinate] {
                    coordinate = candidate;
                }
            } // Split the widest endpoint dimension.
            let split_height = node.height - lowest_height > node.height * 0.02; // Put clearly taller bodies earlier, then subdivide spatially.
            let middle = count / 2; // Every split shrinks both child ranges.
            self.order[begin..begin + count].select_nth_unstable_by(middle, |a, b| {
                // Partition without allocating or sorting an entire subtree.
                let comparison = if split_height {
                    bodies[*b].height.total_cmp(&bodies[*a].height)
                } else {
                    bodies[*a].key[coordinate].total_cmp(&bodies[*b].key[coordinate])
                }; // Select a deterministic partition dimension.
                comparison.then(a.cmp(b)) // Canonical indices break ties consistently.
            }); // Complete the median partition.
            node.count = 0; // Internal nodes do not evaluate bodies themselves.
            if !self.build_node(bodies, begin, middle, stop) {
                return false;
            } // Place the first child immediately after its parent.
            if !self.build_node(bodies, begin + middle, count - middle, stop) {
                return false;
            } // Follow it with the second subtree.
        } // Finish child construction.
        node.end = self.nodes.len(); // A rejected bound can skip this entire contiguous subtree.
        self.nodes[node_index] = node; // Publish the fully initialized node.
        true // Every child range has been constructed successfully.
    } // End balanced-node construction.
    pub(super) fn sample(
        &self,
        bodies: &[Prepared],
        point: [f32; 2],
        softness: f32,
        hint: &mut usize,
        evaluations: &mut usize,
        node_tests: &mut usize,
    ) -> f32 {
        // Query the exact maximum using only conservative rejections.
        let seed = *hint; // A nearby query's winner is only a starting candidate, never an exclusive shortlist.
        let mut height = 0.0; // No supporting body means the unchanged ground plane.
        *hint = usize::MAX; // Do not retain a noncontributing winner.
        if let Some(body) = bodies.get(seed) {
            height = body.sample(point, softness);
            *evaluations += 1;
            if height > 0.0 {
                *hint = seed;
            }
        } // Establish a valid lower bound before traversal.
        let mut node_index = 0; // Start at the root when one exists.
        while node_index < self.nodes.len() {
            // Each node is visited at most once per ground point.
            let node = self.nodes[node_index]; // Copy the fixed-size bound record.
            *node_tests += 1; // Keep query work distinct from lattice-tile work.
            if node.height + 0.000004 <= height || node.upper(point, softness) <= height {
                node_index = node.end;
                continue;
            } // Skip only a mathematically dominated subtree.
            if node.count == 0 {
                node_index += 1;
                continue;
            } // Descend into an internal node's first child.
            for &index in &self.order[node.begin..node.begin + node.count] {
                // Test every potentially winning leaf contributor.
                if index == seed || bodies[index].height + 0.000004 <= height {
                    continue;
                } // Avoid a repeated seed or a conservatively dominated peak.
                *evaluations += 1; // Count each actual analytic primitive evaluation.
                let candidate = bodies[index].sample(point, softness); // Preserve the original f32 primitive arithmetic.
                if candidate > height {
                    height = candidate;
                    *hint = index;
                } // The maximum is order-independent for these finite values.
            } // Finish this leaf.
            node_index = node.end; // Continue with its next sibling or ancestor sibling.
        } // Finish all unpruned subtrees.
        height // Return an actual primitive value, not the bound or an approximation.
    } // End the exact maximum query.
} // End hierarchy methods.
