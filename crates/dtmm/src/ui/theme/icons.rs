use druid::Color;
use usvg::{
    Error, Fill, LineCap, LineJoin, NodeKind, NonZeroPositiveF64, Options, Paint, Stroke, Tree,
};

pub static ALERT_CIRCLE: &str = include_str!("../../../assets/tabler-icons/alert-circle.svg");
pub static ALERT_TRIANGLE: &str = include_str!("../../../assets/tabler-icons/alert-triangle.svg");

pub fn parse_svg(svg: &str) -> Result<Tree, Error> {
    let opt = Options::default();
    Tree::from_str(svg, &opt.to_ref())
}

pub fn recolor_icon(tree: Tree, stroke: bool, color: Color) -> Tree {
    let (red, green, blue, _) = color.as_rgba8();

    let mut children = tree.root.children();
    // The first element is always some kind of background placeholder
    children.next();

    for node in children {
        if let NodeKind::Path(ref mut path) = *node.borrow_mut() {
            if stroke {
                path.stroke = Some(Stroke {
                    paint: Paint::Color(usvg::Color { red, green, blue }),
                    width: NonZeroPositiveF64::new(2.).expect("the value is not zero"),
                    linecap: LineCap::Round,
                    linejoin: LineJoin::Round,
                    ..Default::default()
                });
            } else {
                path.fill = Some(Fill {
                    paint: Paint::Color(usvg::Color { red, green, blue }),
                    ..Default::default()
                });
            }
        }
    }

    tree
}
