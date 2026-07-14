use std::ffi::c_void;
use std::marker::PhantomData;

use crate::geometry::Vec2I;

#[derive(Clone, Copy)]
pub(crate) enum ClipperOperation {
    Union = 0,
    Difference = 1,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct ClipperPoint64 {
    x: i64,
    y: i64,
}

unsafe extern "C" {
    fn gerber_clipper_execute(
        operation: i32,
        subject_points: *const ClipperPoint64,
        subject_offsets: *const usize,
        subject_path_count: usize,
        clip_points: *const ClipperPoint64,
        clip_offsets: *const usize,
        clip_path_count: usize,
    ) -> *mut c_void;
    fn gerber_clipper_tree_delete(tree: *mut c_void);
    fn gerber_clipper_node_child_count(node: *const c_void) -> usize;
    fn gerber_clipper_node_child(node: *const c_void, index: usize) -> *const c_void;
    fn gerber_clipper_node_point_count(node: *const c_void) -> usize;
    fn gerber_clipper_node_point(
        node: *const c_void,
        index: usize,
        point: *mut ClipperPoint64,
    ) -> bool;
}

pub(crate) struct ClipperTree {
    root: *mut c_void,
}

impl ClipperTree {
    pub(crate) fn execute(
        operation: ClipperOperation,
        subject: &[Vec<Vec2I>],
        clip: &[Vec<Vec2I>],
    ) -> Result<Self, &'static str> {
        let (subject_points, subject_offsets) = flatten_paths(subject);
        let (clip_points, clip_offsets) = flatten_paths(clip);
        let root = unsafe {
            gerber_clipper_execute(
                operation as i32,
                subject_points.as_ptr(),
                subject_offsets.as_ptr(),
                subject.len(),
                clip_points.as_ptr(),
                clip_offsets.as_ptr(),
                clip.len(),
            )
        };

        if root.is_null() {
            Err("Clipper2 boolean operation failed")
        } else {
            Ok(Self { root })
        }
    }

    pub(crate) fn root(&self) -> ClipperNode<'_> {
        ClipperNode {
            ptr: self.root,
            marker: PhantomData,
        }
    }
}

impl Drop for ClipperTree {
    fn drop(&mut self) {
        unsafe {
            gerber_clipper_tree_delete(self.root);
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ClipperNode<'a> {
    ptr: *const c_void,
    marker: PhantomData<&'a ClipperTree>,
}

impl ClipperNode<'_> {
    pub(crate) fn child_count(self) -> usize {
        unsafe { gerber_clipper_node_child_count(self.ptr) }
    }

    pub(crate) fn child(self, index: usize) -> Option<Self> {
        let ptr = unsafe { gerber_clipper_node_child(self.ptr, index) };

        if ptr.is_null() {
            None
        } else {
            Some(Self {
                ptr,
                marker: PhantomData,
            })
        }
    }

    pub(crate) fn points(self) -> Vec<Vec2I> {
        let count = unsafe { gerber_clipper_node_point_count(self.ptr) };
        let mut points = Vec::with_capacity(count);

        for index in 0..count {
            let mut point = ClipperPoint64::default();
            let found = unsafe { gerber_clipper_node_point(self.ptr, index, &mut point) };
            assert!(found, "Clipper2 returned an invalid polygon point index");
            points.push(Vec2I::new(
                i32::try_from(point.x).expect("Clipper2 x coordinate exceeded i32"),
                i32::try_from(point.y).expect("Clipper2 y coordinate exceeded i32"),
            ));
        }

        points
    }
}

fn flatten_paths(paths: &[Vec<Vec2I>]) -> (Vec<ClipperPoint64>, Vec<usize>) {
    let point_count = paths.iter().map(Vec::len).sum();
    let mut points = Vec::with_capacity(point_count);
    let mut offsets = Vec::with_capacity(paths.len() + 1);
    offsets.push(0);

    for path in paths {
        points.extend(path.iter().map(|point| ClipperPoint64 {
            x: point.x as i64,
            y: point.y as i64,
        }));
        offsets.push(points.len());
    }

    (points, offsets)
}
