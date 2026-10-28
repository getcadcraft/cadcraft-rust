//! POINTCLOUDATTACH / POINTCLOUD (cloud-cad.js, M4.50) through the command line with a stub host: what is printed,
//! the cloud's state, snapping to it and its picture. Parity with
//! the frozen JS is checked by kdiff-cadcloud.mjs; these keep the paths working without the JS.

use std::cell::RefCell;
use std::rc::Rc;

use cc_cad::Doc;
use cc_cad::entity::Entity;
use cc_cad_geom::geom::Bounds;
use cc_engine::cloud::CloudAttach;
use cc_engine::{Engine, Host, Lang, Pick, PickedFile};
use cc_geom::P2;

#[derive(Default)]
struct Log {
    printed: Vec<String>,
    files: Vec<Option<PickedFile>>,
    zooms: Vec<Bounds>,
    blobs: Vec<(String, Vec<u8>)>,
}

struct Stub(Rc<RefCell<Log>>, Rc<RefCell<CloudAttach>>);

impl Host for Stub {
    fn print(&mut self, s: &str) {
        self.0.borrow_mut().printed.push(s.into());
    }
    fn prompt(&mut self, _s: &str) {}
    fn cursor(&self) -> Option<P2> {
        None
    }
    fn selection(&self) -> Vec<u64> {
        Vec::new()
    }
    fn set_selection(&mut self, _ids: &[u64]) {}
    fn pick_at(&self, _doc: &Doc, _p: P2) -> Option<u64> {
        None
    }
    fn pick_window(&self, _doc: &Doc, _w: &Bounds, _crossing: bool) -> Vec<u64> {
        Vec::new()
    }
    fn preview(&mut self, _ents: Vec<Entity>) {}
    fn zoom_extents(&mut self) {}
    fn zoom_window(&mut self, _a: P2, _b: P2, _exact: bool) {}
    fn now_ms(&self) -> Option<f64> {
        Some(1_760_184_000_000.0)
    }
    fn can_save_blob(&self) -> bool {
        true
    }
    fn save_blob(&mut self, name: &str, data: &[u8], _mime: &str) {
        self.0.borrow_mut().blobs.push((name.into(), data.to_vec()));
    }
    fn cloud(&self) -> Option<Rc<RefCell<CloudAttach>>> {
        Some(Rc::clone(&self.1))
    }
    fn pick_cloud(&mut self) -> Pick<Option<PickedFile>> {
        let f = self.0.borrow_mut().files.pop();
        Pick::Now(f.flatten())
    }
    fn zoom_cloud(&mut self, b: Bounds) {
        self.0.borrow_mut().zooms.push(b);
    }
}

fn engine(doc: Doc, files: Vec<Option<PickedFile>>) -> (Engine, Rc<RefCell<Log>>, Rc<RefCell<CloudAttach>>) {
    let log = Rc::new(RefCell::new(Log { files, ..Log::default() }));
    let pc: Rc<RefCell<CloudAttach>> = Rc::default();
    let cat: serde_json::Value = serde_json::from_str(include_str!("../../../assets/lang/cadcraft/en.json")).unwrap_or_default();
    (Engine::new(doc, Box::new(Stub(Rc::clone(&log), Rc::clone(&pc))), Lang::new(&cat)), log, pc)
}

/// A room of points 6 × 4 × 3 (a wall every 0.5 along x at y = 0, a column of points, colours by height).
fn room_pts() -> PickedFile {
    let mut s = String::from("# a room\n");
    for i in 0..=12 {
        for k in 0..=6 {
            let (x, z) = (f64::from(i) * 0.5, f64::from(k) * 0.5);
            s.push_str(&format!("{} 0 {z} 10 {} 0 0\n", x + 100.0, k * 40));
            s.push_str(&format!("{} 4 {z} 20 0 {} 0\n", x + 100.0, k * 40));
        }
    }
    PickedFile { name: "room.xyz".into(), bytes: s.into_bytes() }
}

#[test]
fn attach_at_its_own_coordinates_then_every_option() {
    let (mut e, log, pc) = engine(Doc::new(), vec![Some(room_pts())]);
    e.text("PCATTACH");
    e.text("");
    let printed = log.borrow().printed.join("\n");
    assert!(printed.contains("Reading room.xyz…"), "{printed}");
    assert!(printed.contains("room.xyz: showing 182 of 182 points (0.0 s)."), "{printed}");
    assert!(printed.contains("1 line was not x y z numbers and was skipped."), "{printed}");
    assert!(printed.contains("A building-size scan"), "a 3 tall, 6 across cloud starts sliced: {printed}");
    assert_eq!(log.borrow().zooms.len(), 1);
    {
        let p = pc.borrow();
        assert!(p.has() && p.slice.on && p.insert.is_none());
        assert_eq!(p.bbox(""), Some([100.0, 0.0, 106.0, 4.0]));
    }
    // the slice at 1.2 (± 0.05): no point there in this cloud; every point once it is off
    assert_eq!(pc.borrow_mut().snap(P2::new(101.02, 0.01), 0.1, ""), None);
    e.text("PC");
    e.text("S");
    assert_eq!(pc.borrow_mut().snap(P2::new(101.02, 0.01), 0.1, ""), Some(P2::new(101.0, 0.0)));
    e.text("C");
    e.text("H");
    assert_eq!(pc.borrow().color, "height");
    e.text("SI");
    e.text("9");
    e.text("SI");
    e.text("3");
    assert_eq!(pc.borrow().size, 3.0);
    let r = pc.borrow_mut().draw([10.0, 0.0, 0.0, -10.0, -995.0, 45.0], 80.0, 50.0, 1.0, "").cloned();
    let r = r.unwrap_or_default();
    assert_eq!((r.w, r.h), (80, 50));
    assert!(r.px.iter().any(|p| *p != 0), "points drawn");
    e.text("V");
    assert!(pc.borrow_mut().draw([1.0, 0.0, 0.0, 1.0, 0.0, 0.0], 10.0, 10.0, 1.0, "").is_none(), "hidden: nothing drawn");
    e.text("D");
    assert!(!pc.borrow().has());
    let printed = log.borrow().printed.join("\n");
    assert!(printed.contains("Type a size from 1 to 8."), "{printed}");
    assert!(printed.contains("room.xyz detached."), "{printed}");
    e.text("PC");
    assert!(log.borrow().printed.last().is_some_and(|l| l.starts_with("No point cloud is attached")));
}

#[test]
fn a_corner_point_and_refusals() {
    let bad = PickedFile { name: "notes.txt".into(), bytes: b"hello".to_vec() };
    let cut = PickedFile { name: "cut.las".into(), bytes: b"LASF\x00\x00".to_vec() };
    // popped from the end: the room first, then the refusals, then nothing picked
    let (mut e, log, pc) = engine(Doc::new(), vec![None, Some(cut), Some(bad), Some(room_pts())]);
    e.text("POINTCLOUDATTACH");
    e.text("10,20");
    assert_eq!(pc.borrow().insert, Some(P2::new(10.0, 20.0)));
    assert_eq!(pc.borrow().bbox(""), Some([10.0, 20.0, 16.0, 24.0]));
    e.text("PCATTACH");
    e.text("PCATTACH");
    e.text("PCATTACH");
    let printed = log.borrow().printed.join("\n");
    assert!(printed.contains("Could not open notes.txt: notes.txt is not a point cloud CadCraft reads"), "{printed}");
    assert!(printed.contains("Could not open cut.las:"), "{printed}");
    assert!(pc.borrow().has(), "a refused file leaves the cloud attached");
}
