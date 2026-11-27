use std::collections::HashMap;

pub struct SpatialGrid {
    cell_size: f32,
    cells: HashMap<(i32, i32), Vec<(usize, f32, f32)>>,
}

impl SpatialGrid {
    pub fn new(cell_size: f32) -> Self {
        Self {
            cell_size: cell_size.max(1.0),
            cells: HashMap::new(),
        }
    }

    pub fn insert(&mut self, idx: usize, x: f32, y: f32) {
        let cx = (x / self.cell_size).floor() as i32;
        let cy = (y / self.cell_size).floor() as i32;
        self.cells.entry((cx, cy)).or_default().push((idx, x, y));
    }

    pub fn query_radius(&self, x: f32, y: f32, radius: f32, out: &mut Vec<(usize, f32, f32)>) {
        out.clear();
        let min_cx = ((x - radius) / self.cell_size).floor() as i32;
        let max_cx = ((x + radius) / self.cell_size).floor() as i32;
        let min_cy = ((y - radius) / self.cell_size).floor() as i32;
        let max_cy = ((y + radius) / self.cell_size).floor() as i32;
        let r2 = radius * radius;

        for cx in min_cx..=max_cx {
            for cy in min_cy..=max_cy {
                if let Some(bucket) = self.cells.get(&(cx, cy)) {
                    for &(idx, bx, by) in bucket {
                        let dx = bx - x;
                        let dy = by - y;
                        if dx * dx + dy * dy <= r2 {
                            out.push((idx, bx, by));
                        }
                    }
                }
            }
        }
    }
}
