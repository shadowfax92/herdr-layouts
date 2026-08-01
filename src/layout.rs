use anyhow::{bail, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SplitDirection {
    Right,
    Down,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LayoutNode {
    Pane {
        pane_id: Option<String>,
    },
    Split {
        direction: SplitDirection,
        ratio: f32,
        first: Box<LayoutNode>,
        second: Box<LayoutNode>,
    },
}

#[derive(Debug, Deserialize)]
pub struct LayoutDescription {
    pub tab_id: String,
    pub root: LayoutNode,
}

#[derive(Debug, PartialEq)]
pub struct RatioUpdate {
    pub path: Vec<bool>,
    pub ratio: f32,
}

pub fn equalize_updates(root: &LayoutNode, pane_id: &str) -> Result<Vec<RatioUpdate>> {
    let mut pane_path = Vec::new();
    if !find_pane_path(root, pane_id, &mut pane_path) {
        bail!("focused pane {pane_id} is absent from its exported layout");
    }
    if pane_path.is_empty() {
        return Ok(Vec::new());
    }

    let mut top_path = pane_path[..pane_path.len() - 1].to_vec();
    let direction = split_direction_at(root, &top_path)
        .ok_or_else(|| anyhow::anyhow!("focused pane has no parent split"))?;
    while !top_path.is_empty() {
        let ancestor_path = &top_path[..top_path.len() - 1];
        if split_direction_at(root, ancestor_path) == Some(direction) {
            top_path.pop();
        } else {
            break;
        }
    }

    let top =
        node_at(root, &top_path).ok_or_else(|| anyhow::anyhow!("equalization split is missing"))?;
    let mut updates = Vec::new();
    collect_updates(top, direction, &mut top_path, &mut updates);
    Ok(updates)
}

fn find_pane_path(node: &LayoutNode, pane_id: &str, path: &mut Vec<bool>) -> bool {
    match node {
        LayoutNode::Pane { pane_id: current } => current.as_deref() == Some(pane_id),
        LayoutNode::Split { first, second, .. } => {
            path.push(false);
            if find_pane_path(first, pane_id, path) {
                return true;
            }
            path.pop();
            path.push(true);
            if find_pane_path(second, pane_id, path) {
                return true;
            }
            path.pop();
            false
        }
    }
}

fn node_at<'a>(node: &'a LayoutNode, path: &[bool]) -> Option<&'a LayoutNode> {
    if path.is_empty() {
        return Some(node);
    }
    match node {
        LayoutNode::Pane { .. } => None,
        LayoutNode::Split { first, second, .. } => {
            node_at(if path[0] { second } else { first }, &path[1..])
        }
    }
}

fn split_direction_at(node: &LayoutNode, path: &[bool]) -> Option<SplitDirection> {
    match node_at(node, path)? {
        LayoutNode::Split { direction, .. } => Some(*direction),
        LayoutNode::Pane { .. } => None,
    }
}

fn axis_units(node: &LayoutNode, direction: SplitDirection) -> usize {
    match node {
        LayoutNode::Split {
            direction: current,
            first,
            second,
            ..
        } if *current == direction => axis_units(first, direction) + axis_units(second, direction),
        _ => 1,
    }
}

fn collect_updates(
    node: &LayoutNode,
    direction: SplitDirection,
    path: &mut Vec<bool>,
    updates: &mut Vec<RatioUpdate>,
) {
    let LayoutNode::Split {
        direction: current,
        ratio,
        first,
        second,
    } = node
    else {
        return;
    };
    if *current != direction {
        return;
    }

    let first_units = axis_units(first, direction);
    let second_units = axis_units(second, direction);
    let target = (first_units as f32 / (first_units + second_units) as f32).clamp(0.1, 0.9);
    if (*ratio - target).abs() > 0.0001 {
        updates.push(RatioUpdate {
            path: path.clone(),
            ratio: target,
        });
    }

    path.push(false);
    collect_updates(first, direction, path, updates);
    path.pop();
    path.push(true);
    collect_updates(second, direction, path, updates);
    path.pop();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane(id: &str) -> LayoutNode {
        LayoutNode::Pane {
            pane_id: Some(id.into()),
        }
    }

    fn split(
        direction: SplitDirection,
        ratio: f32,
        first: LayoutNode,
        second: LayoutNode,
    ) -> LayoutNode {
        LayoutNode::Split {
            direction,
            ratio,
            first: Box::new(first),
            second: Box::new(second),
        }
    }

    #[test]
    fn equalizes_a_horizontal_chain() {
        let root = split(
            SplitDirection::Right,
            0.5,
            pane("a"),
            split(SplitDirection::Right, 0.8, pane("b"), pane("c")),
        );

        assert_eq!(
            equalize_updates(&root, "c").unwrap(),
            [
                RatioUpdate {
                    path: vec![],
                    ratio: 1.0 / 3.0,
                },
                RatioUpdate {
                    path: vec![true],
                    ratio: 0.5,
                }
            ]
        );
    }

    #[test]
    fn preserves_perpendicular_splits() {
        let root = split(
            SplitDirection::Right,
            0.7,
            split(SplitDirection::Down, 0.7, pane("top"), pane("bottom")),
            pane("right"),
        );

        assert_eq!(
            equalize_updates(&root, "top").unwrap(),
            [RatioUpdate {
                path: vec![false],
                ratio: 0.5,
            }]
        );
        assert_eq!(
            equalize_updates(&root, "right").unwrap(),
            [RatioUpdate {
                path: vec![],
                ratio: 0.5,
            }]
        );
    }

    #[test]
    fn leaves_a_single_pane_unchanged() {
        assert!(equalize_updates(&pane("only"), "only").unwrap().is_empty());
    }
}
