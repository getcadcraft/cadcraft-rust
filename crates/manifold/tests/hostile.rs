//! Hostile input: malformed, degenerate, non-manifold and oversized meshes give an error status (or an empty
//! result), never a panic, a hang or an unbounded allocation. Regression cases for crashes found by fuzzing go here
//! too (fuzz/fuzz_targets/manifold_bool.rs).

use cc_manifold::{Manifold, Mesh, Status};

fn cube(o: f32, s: f32) -> Mesh {
    let mut v = Vec::new();
    for x in [0.0, s] {
        for y in [0.0, s] {
            for z in [0.0, s] {
                v.extend([o + x, o + y, o + z]);
            }
        }
    }
    let t = vec![1, 0, 4, 2, 4, 0, 1, 3, 0, 3, 1, 5, 3, 2, 0, 3, 7, 2, 5, 4, 6, 5, 1, 4, 6, 4, 2, 7, 6, 2, 7, 3, 5, 7, 5, 6];
    Mesh::new(3, v, t)
}

fn all_ops(a: &Manifold, b: &Manifold) {
    for r in [a.subtract(b), a.add(b), a.intersect(b), Manifold::union(&[a.clone(), b.clone(), a.clone()])] {
        let _ = r.status();
        let m = r.get_mesh();
        assert_eq!(m.vert_properties.len() % 3, 0);
    }
}

#[test]
fn statuses_in_cpp_order() {
    let ok = cube(0.0, 1.0);
    assert!(Manifold::from_mesh(&ok).is_ok());

    let empty = Mesh::new(3, vec![], vec![]);
    assert!(Manifold::from_mesh(&empty).is_ok_and(|m| m.is_empty()));

    let mut open = cube(0.0, 1.0);
    open.tri_verts.truncate(33);
    assert_eq!(Manifold::from_mesh(&open).unwrap_err(), Status::NotManifold);

    let mut oob = cube(0.0, 1.0);
    oob.tri_verts[4] = 99;
    assert_eq!(Manifold::from_mesh(&oob).unwrap_err(), Status::VertexOutOfBounds);

    let mut nan = cube(0.0, 1.0);
    nan.vert_properties[5] = f32::NAN;
    assert_eq!(Manifold::from_mesh(&nan).unwrap_err(), Status::NonFiniteVertex);

    let mut two = cube(0.0, 1.0);
    two.num_prop = 2;
    assert_eq!(Manifold::from_mesh(&two).unwrap_err(), Status::MissingPositionProperties);

    let mut zero = cube(0.0, 1.0);
    zero.num_prop = 0;
    assert_eq!(Manifold::from_mesh(&zero).unwrap_err(), Status::MissingPositionProperties);

    let mut merge = cube(0.0, 1.0);
    merge.merge_from_vert = vec![0];
    assert_eq!(Manifold::from_mesh(&merge).unwrap_err(), Status::MergeVectorsDifferentLengths);
    merge.merge_to_vert = vec![100];
    assert_eq!(Manifold::from_mesh(&merge).unwrap_err(), Status::MergeIndexOutOfBounds);

    let mut runs = cube(0.0, 1.0);
    runs.run_original_id = vec![1, 2, 3];
    runs.run_index = vec![0, 36];
    assert_eq!(Manifold::from_mesh(&runs).unwrap_err(), Status::RunIndexWrongLength);
    runs.run_index = vec![0, 3, 999];
    assert_eq!(Manifold::from_mesh(&runs).unwrap_err(), Status::RunIndexWrongLength);

    let mut faces = cube(0.0, 1.0);
    faces.face_id = vec![0; 5];
    assert_eq!(Manifold::from_mesh(&faces).unwrap_err(), Status::FaceIDWrongLength);
}

#[test]
fn merge_with_bad_indices_changes_nothing() {
    // the C++ reads out of bounds here (the wasm build traps or reads garbage)
    let mut m = cube(0.0, 1.0);
    m.tri_verts[7] = 9999;
    m.tri_verts.truncate(30);
    assert!(!m.merge());
    let mut m = cube(0.0, 1.0);
    m.num_prop = 1;
    assert!(!m.merge());
    let mut m = cube(0.0, 1.0);
    m.merge_from_vert = vec![50];
    m.merge_to_vert = vec![0];
    assert!(!m.merge());
}

#[test]
fn degenerate_inputs_never_panic() {
    let a = Manifold::from_mesh(&cube(0.0, 1.0)).unwrap();
    // flat, inside out, duplicated, tiny, huge, coincident
    let mut flat = cube(0.0, 1.0);
    for i in (2..flat.vert_properties.len()).step_by(3) {
        flat.vert_properties[i] = 0.5;
    }
    let mut inside_out = cube(0.25, 0.5);
    for t in inside_out.tri_verts.chunks_mut(3) {
        t.swap(1, 2);
    }
    let mut doubled = cube(0.2, 0.5);
    let n = u32::try_from(doubled.num_vert()).unwrap();
    let extra = cube(0.2, 0.5);
    doubled.vert_properties.extend(extra.vert_properties);
    doubled.tri_verts.extend(extra.tri_verts.iter().map(|v| v + n));
    for m in [flat, inside_out, doubled, cube(0.5, 1e-30), cube(-3e38, 3e38), cube(0.0, 1.0), cube(1.0, 1.0)] {
        let mut m = m;
        m.merge();
        let b = Manifold::from_mesh_unchecked(&m);
        all_ops(&a, &b);
        all_ops(&b, &a);
        all_ops(&b, &b);
    }
}

#[test]
fn soups_and_pinches_never_panic() {
    // every triangle on its own vertices, then merged with a large tolerance: pinched and non-manifold topology
    let c = cube(0.0, 1.0);
    let mut soup = Mesh::new(3, Vec::new(), Vec::new());
    for (k, &v) in c.tri_verts.iter().enumerate() {
        let v = usize::try_from(v).unwrap();
        soup.vert_properties.extend_from_slice(&c.vert_properties[3 * v..3 * v + 3]);
        soup.tri_verts.push(u32::try_from(k).unwrap());
    }
    soup.tolerance = 0.6;
    soup.merge();
    let b = Manifold::from_mesh_unchecked(&soup);
    let a = Manifold::from_mesh(&cube(0.3, 1.0)).unwrap();
    all_ops(&a, &b);
}

#[test]
fn oversized_input_is_refused() {
    let mut m = cube(0.0, 1.0);
    m.num_prop = cc_manifold::MAX_NUM_PROP + 1;
    m.vert_properties = vec![0.5; m.vert_properties.len() * 30];
    assert!(Manifold::from_mesh(&m).is_err());
}

#[test]
fn manifold_is_send_and_sync() {
    // desktop callers may build in one thread and evaluate in another
    fn check<T: Send + Sync>() {}
    check::<Manifold>();
    check::<Mesh>();
}
