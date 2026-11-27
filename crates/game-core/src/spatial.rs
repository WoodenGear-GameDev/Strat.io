/// Flat 2D spatial hash grid with zero-allocation per-tick clearing.
pub struct SpatialGrid {
    origin: f32,
    inv_cell_size: f32,
    cols: usize,
    cells: Vec<Vec<(usize, f32, f32)>>,
    touched: Vec<usize>,
}

impl SpatialGrid {
    pub fn new(cell_size: f32) -> Self {
        Self::new_for_world(3400.0, cell_size)
    }

    pub fn new_for_world(map_radius: f32, cell_size: f32) -> Self {
        let cs = cell_size.max(1.0);
        let extent = map_radius.max(512.0) + cs * 2.0;
        let cols = ((extent * 2.0) / cs).ceil() as usize + 1;
        let total = cols * cols;
        Self {
            origin: -extent,
            inv_cell_size: 1.0 / cs,
            cols,
            cells: vec![Vec::new(); total],
            touched: Vec::with_capacity(256),
        }
    }

    #[inline]
    pub fn clear(&mut self) {
        for &ci in &self.touched {
            self.cells[ci].clear();
        }
        self.touched.clear();
    }

    #[inline]
    fn cell_coord(&self, v: f32) -> usize {
        let max_idx = (self.cols - 1) as isize;
        (((v - self.origin) * self.inv_cell_size).floor() as isize).clamp(0, max_idx) as usize
    }

    #[inline]
    pub fn insert(&mut self, idx: usize, x: f32, y: f32) {
        let cx = self.cell_coord(x);
        let cy = self.cell_coord(y);
        let ci = cy * self.cols + cx;
        let bucket = &mut self.cells[ci];
        if bucket.is_empty() {
            self.touched.push(ci);
        }
        bucket.push((idx, x, y));
    }

    #[inline]
    pub fn for_each_in_radius(
        &self,
        x: f32,
        y: f32,
        radius: f32,
        mut f: impl FnMut(usize, f32, f32, f32),
    ) {
        let min_cx = self.cell_coord(x - radius);
        let max_cx = self.cell_coord(x + radius);
        let min_cy = self.cell_coord(y - radius);
        let max_cy = self.cell_coord(y + radius);
        let r2 = radius * radius;
        let cols = self.cols;

        for cy in min_cy..=max_cy {
            let row = cy * cols;
            for cx in min_cx..=max_cx {
                let bucket = &self.cells[row + cx];
                for &(idx, bx, by) in bucket {
                    let dx = bx - x;
                    let dy = by - y;
                    let d2 = dx * dx + dy * dy;
                    if d2 <= r2 {
                        f(idx, bx, by, d2);
                    }
                }
            }
        }
    }

    /// Like `for_each_in_radius`, but stops early as soon as `f` returns `false`.
    #[inline]
    pub fn for_each_in_radius_until(
        &self,
        x: f32,
        y: f32,
        radius: f32,
        mut f: impl FnMut(usize, f32, f32, f32) -> bool,
    ) {
        let min_cx = self.cell_coord(x - radius);
        let max_cx = self.cell_coord(x + radius);
        let min_cy = self.cell_coord(y - radius);
        let max_cy = self.cell_coord(y + radius);
        let r2 = radius * radius;
        let cols = self.cols;

        for cy in min_cy..=max_cy {
            let row = cy * cols;
            for cx in min_cx..=max_cx {
                let bucket = &self.cells[row + cx];
                for &(idx, bx, by) in bucket {
                    let dx = bx - x;
                    let dy = by - y;
                    let d2 = dx * dx + dy * dy;
                    if d2 <= r2 && !f(idx, bx, by, d2) {
                        return;
                    }
                }
            }
        }
    }

    pub fn query_radius(&self, x: f32, y: f32, radius: f32, out: &mut Vec<(usize, f32, f32)>) {
        out.clear();
        let min_cx = self.cell_coord(x - radius);
        let max_cx = self.cell_coord(x + radius);
        let min_cy = self.cell_coord(y - radius);
        let max_cy = self.cell_coord(y + radius);
        let r2 = radius * radius;
        let cols = self.cols;

        for cy in min_cy..=max_cy {
            let row = cy * cols;
            for cx in min_cx..=max_cx {
                let bucket = &self.cells[row + cx];
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
