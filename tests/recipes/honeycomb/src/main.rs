// A honeycomb panel: hexagonal pockets on a staggered grid, skipping any
// cell that would cut through the border.
use kit::{Session, ops};

fn main() {
    let mut s = Session::start();
    let (w, h) = (s.param_f64("width", 80.0), s.param_f64("height", 50.0));
    let cell = s.param_f64("cell", 10.0);
    let wall = s.param_f64("wall", 1.2);
    let border = 3.0;
    let part = s.new_box([w, h, 3.0]);
    let dx = cell + wall;
    let dy = dx * 3f64.sqrt() / 2.0;
    let mut row = 0;
    let mut y = border + cell / 2.0;
    let mut cut = 0;
    while y + cell / 2.0 <= h - border {
        let mut x = border + cell / 2.0 + if row % 2 == 1 { dx / 2.0 } else { 0.0 };
        while x + cell / 2.0 <= w - border {
            if s.apply(&part, &ops::polygon_pocket([x, y, 3.0], 6, cell, 2.0)) {
                cut += 1;
            }
            x += dx;
        }
        y += dy;
        row += 1;
    }
    eprintln!("{cut} cells");
    s.done(&part);
}
