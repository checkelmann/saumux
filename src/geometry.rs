use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

impl Rect {
    pub fn contains(self, x: u16, y: u16) -> bool {
        x >= self.x
            && y >= self.y
            && x < self.x.saturating_add(self.w)
            && y < self.y.saturating_add(self.h)
    }

    pub fn fit(self, cols: u16, rows: u16) -> Self {
        let w = self.w.clamp(4, cols.max(4));
        let h = self.h.clamp(3, rows.saturating_sub(2).max(3));
        Self {
            x: self.x.min(cols.saturating_sub(w)),
            y: self.y.clamp(1, rows.saturating_sub(h + 1).max(1)),
            w,
            h,
        }
    }

    pub fn desktop(cols: u16, rows: u16) -> Self {
        Self {
            x: 0,
            y: 1,
            w: cols.max(4),
            h: rows.saturating_sub(2).max(3),
        }
    }

    pub fn inner(self) -> (u16, u16) {
        (
            self.w.saturating_sub(2).max(1),
            self.h.saturating_sub(2).max(1),
        )
    }
}

pub fn tile(count: usize, cols: u16, rows: u16) -> Vec<Rect> {
    if count == 0 {
        return vec![];
    }
    let across = (count as f64).sqrt().ceil() as usize;
    let down = count.div_ceil(across);
    let height = rows.saturating_sub(2);
    (0..count)
        .map(|i| {
            let y = i / across;
            let row_count = across.min(count - y * across);
            let x = i % across;
            let left = cols as usize * x / row_count;
            let right = cols as usize * (x + 1) / row_count;
            let top = height as usize * y / down;
            let bottom = height as usize * (y + 1) / down;
            Rect {
                x: left as u16,
                y: 1 + top as u16,
                w: (right - left) as u16,
                h: (bottom - top) as u16,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tiling_covers_the_desktop_once() {
        for count in 1..12 {
            let rects = tile(count, 103, 35);
            for y in 1..34 {
                for x in 0..103 {
                    assert_eq!(rects.iter().filter(|r| r.contains(x, y)).count(), 1);
                }
            }
        }
    }
    #[test]
    fn fitting_keeps_the_title_reachable() {
        let r = Rect {
            x: 400,
            y: 400,
            w: 80,
            h: 30,
        }
        .fit(60, 16);
        assert_eq!(
            r,
            Rect {
                x: 0,
                y: 1,
                w: 60,
                h: 14
            }
        );
    }
}
