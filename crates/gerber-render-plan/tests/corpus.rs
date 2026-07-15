use std::path::PathBuf;

use gerber_parse::gerber_file_image::GerberFileImage;
use gerber_parse::readgerb::{load_gerber_file, parse_gerber_str};
use gerber_render_plan::{FillSource, Polarity, RenderGeometry, RenderPlan};

fn corpus_file(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("test-corpus")
        .join("tracespace-v5")
        .join("input")
        .join(name)
}

fn load_corpus(name: &str) -> GerberFileImage {
    let path = corpus_file(name);
    load_gerber_file(&path.to_string_lossy())
        .unwrap_or_else(|error| panic!("failed to load {}: {error:?}", path.display()))
}

#[test]
fn full_circle_fixture_remains_an_analytic_arc() {
    let image = load_corpus("gerbers__arc-strokes__full-circle__gbr.gbr");
    let plan = RenderPlan::from_image(&image).unwrap();

    assert!(plan.operations.iter().any(|operation| {
        matches!(
            &operation.geometry,
            RenderGeometry::StrokeArc(arc) if arc.full_circle
        )
    }));
}

#[test]
fn zero_length_round_stroke_is_not_dropped() {
    let image = load_corpus("gerbers__strokes__circle-tool-zero-length__gbr.gbr");
    let plan = RenderPlan::from_image(&image).unwrap();

    assert!(plan.operations.iter().any(|operation| {
        matches!(
            &operation.geometry,
            RenderGeometry::StrokeLine(line)
                if line.start == line.end && line.width > 0
        )
    }));
}

#[test]
fn zero_length_rectangular_stroke_remains_a_fill() {
    let image = load_corpus("gerbers__strokes__rect-tool-zero-length__gbr.gbr");
    let plan = RenderPlan::from_image(&image).unwrap();

    assert!(plan.operations.iter().any(|operation| {
        matches!(
            &operation.geometry,
            RenderGeometry::FillPath(fill)
                if fill.source == FillSource::ExpandedStroke
                    && !fill.polygons.is_empty()
        )
    }));
}

#[test]
fn polarity_fixture_preserves_ordered_dark_and_clear_operations() {
    let image = load_corpus("gerbers__step-repeats__multi-polarity-over-self__gbr.gbr");
    let plan = RenderPlan::from_image(&image).unwrap();

    assert!(
        plan.operations
            .windows(2)
            .all(|pair| pair[0].draw_order < pair[1].draw_order)
    );
    assert!(
        plan.operations
            .iter()
            .any(|operation| operation.effective_polarity == Polarity::Positive)
    );
    assert!(
        plan.operations
            .iter()
            .any(|operation| operation.effective_polarity == Polarity::Negative)
    );
}

#[test]
fn easyeda_outline_keeps_four_small_quadrant_arcs() {
    let mut image = GerberFileImage::default();
    parse_gerber_str(
        &mut image,
        "%FSLAX45Y45*%%MOMM*%%ADD10C,0.254*%G75*\
         G54D10*G01X13393825Y-8352155D02*\
         G01X-3991840Y-8352155D01*\
         G02X-4296640Y-8047355I0J304800D01*\
         G01X-4296640Y13830300D01*\
         G02X-3991840Y14135100I304800J0D01*\
         G01X13393825Y14135100D01*\
         G02X13698625Y13830300I0J-304800D01*\
         G01X13698625Y-8047355D01*\
         G02X13393825Y-8352155I-304800J0D01*M02*",
    );

    let plan = RenderPlan::from_image(&image).unwrap();
    let arcs = plan
        .operations
        .iter()
        .filter_map(|operation| match &operation.geometry {
            RenderGeometry::StrokeArc(arc) => Some((arc, operation.bbox)),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(plan.operations.len(), 8);
    assert_eq!(arcs.len(), 4);

    for (arc, bbox) in arcs {
        assert!(!arc.full_circle);
        assert!(bbox.width() < 1_000_000);
        assert!(bbox.height() < 1_000_000);
    }
}
