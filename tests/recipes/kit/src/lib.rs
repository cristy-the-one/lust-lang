//! A recipe's side of the JSON-lines protocol (tests/recipes/README.md).
//! No dependencies: numbers print with `{}`, which is rustc's shortest
//! round-trip form under both compilers.
use std::io::Write;

/// One run: the parameters the host sent and the call counter.
pub struct Session {
    params: Vec<(String, f64)>,
    next_id: u64,
}

/// A solid the host holds, by handle.
pub struct Part {
    pub handle: String,
}

/// A reply: `ok` and the raw `result` or `error` text.
pub struct Reply {
    pub ok: bool,
    pub body: String,
}

impl Session {
    /// Reads the start line: `{"v":1,"params":{"name":number,...}}`.
    pub fn start() -> Session {
        let line = read_line();
        let mut params = Vec::new();
        if let Some(at) = line.find("\"params\":{") {
            let rest = &line[at + 10..];
            let body = &rest[..rest.find('}').unwrap_or(rest.len())];
            for pair in body.split(',') {
                if let Some((k, v)) = pair.split_once(':') {
                    let key = k.trim().trim_matches('"').to_string();
                    if let Ok(n) = v.trim().parse::<f64>() {
                        params.push((key, n));
                    }
                }
            }
        }
        Session { params, next_id: 1 }
    }

    pub fn param_f64(&self, name: &str, default: f64) -> f64 {
        self.params.iter().find(|(k, _)| k == name).map_or(default, |(_, v)| *v)
    }

    pub fn param_u32(&self, name: &str, default: u32) -> u32 {
        self.param_f64(name, default as f64) as u32
    }

    /// Sends one call and waits for its reply. The call carries the
    /// recipe line that made it, so the host can point at it.
    #[track_caller]
    pub fn call(&mut self, call: &str, args: &str) -> Reply {
        let at = std::panic::Location::caller();
        let id = self.next_id;
        self.next_id += 1;
        let mut out = std::io::stdout();
        writeln!(out, "{{\"id\":{id},\"call\":\"{call}\",\"args\":{args},\"at\":{{\"line\":{},\"col\":{}}}}}", at.line(), at.column()).unwrap();
        out.flush().unwrap();
        let line = read_line();
        let ok = line.contains("\"ok\":true");
        let body = match line.find(if ok { "\"result\":" } else { "\"error\":" }) {
            Some(i) => line[i..].split_once(':').map(|(_, v)| v.trim_end_matches('}').to_string()).unwrap_or_default(),
            None => String::new(),
        };
        Reply { ok, body }
    }

    #[track_caller]
    pub fn new_box(&mut self, size: [f64; 3]) -> Part {
        let r = self.call("solid_new", &format!("{{\"kind\":\"box\",\"size\":{}}}", vec3(size)));
        Part { handle: r.body.trim_matches('"').to_string() }
    }

    #[track_caller]
    pub fn apply(&mut self, part: &Part, op: &str) -> bool {
        self.call("apply", &format!("{{\"object\":\"{}\",\"op\":{op}}}", part.handle)).ok
    }

    /// The solid's volume in mm³, as the host measures it.
    #[track_caller]
    pub fn volume(&mut self, part: &Part) -> f64 {
        self.call("measure", &format!("{{\"object\":\"{}\",\"what\":\"volume\"}}", part.handle)).body.parse().unwrap_or(0.0)
    }

    pub fn done(self, part: &Part) {
        println!("{{\"done\":{{\"object\":\"{}\",\"calls\":{}}}}}", part.handle, self.next_id - 1);
    }
}

fn read_line() -> String {
    let mut s = String::new();
    std::io::stdin().read_line(&mut s).unwrap();
    s.trim_end().to_string()
}

pub fn vec3(v: [f64; 3]) -> String {
    format!("[{},{},{}]", v[0], v[1], v[2])
}

/// Op builders: the JSON of one op, as the host's `apply` takes it.
pub mod ops {
    use super::vec3;

    pub fn hole_down(at: [f64; 3], radius: f64, depth: Option<f64>) -> String {
        match depth {
            Some(d) => format!("{{\"kind\":\"hole\",\"at\":{},\"radius\":{radius},\"depth\":{d}}}", vec3(at)),
            None => format!("{{\"kind\":\"hole\",\"at\":{},\"radius\":{radius},\"through\":true}}", vec3(at)),
        }
    }

    pub fn fillet_top(radius: f64) -> String {
        format!("{{\"kind\":\"fillet\",\"edges\":\"top\",\"radius\":{radius}}}")
    }

    pub fn polygon_pocket(center: [f64; 3], sides: u32, across: f64, depth: f64) -> String {
        format!("{{\"kind\":\"pocket\",\"sides\":{sides},\"center\":{},\"across\":{across},\"depth\":{depth}}}", vec3(center))
    }
}
