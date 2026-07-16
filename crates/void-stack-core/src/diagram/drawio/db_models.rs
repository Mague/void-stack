//! Draw.io DB models page generation with FK-proximity layout.

use super::super::db_models as db_scan;
use super::super::ir::{ModelLink, is_fk_field};

use super::common::*;

pub(crate) fn render_db_models_page(
    all_models: &[db_scan::DbModel],
    links: &[ModelLink],
    xml: &mut String,
) {
    if all_models.is_empty() {
        return;
    }

    // FK relationships come pre-computed from the shared IR.
    let model_count = all_models.len();
    let fk_links: Vec<(usize, usize)> = links.iter().map(|l| (l.from, l.to)).collect();
    let mut model_fk_targets: Vec<Vec<(String, usize)>> = vec![Vec::new(); model_count];
    for l in links {
        if l.from < model_count && l.to < model_count {
            model_fk_targets[l.from].push((l.field.clone(), l.to));
        }
    }

    // ── Order models by connectivity (BFS from most-connected) ──
    let ordered = bfs_order(model_count, &fk_links);

    // ── Layout ──
    let mut ids = IdGen::new();
    let cols = 4.min(model_count).max(1);
    let card_w: u32 = 240;
    let row_h: u32 = 22;
    let header_h: u32 = 30;
    let pad: u32 = 10;
    let spacing_x: u32 = 60;
    let spacing_y: u32 = 50;

    let card_heights: Vec<u32> = all_models
        .iter()
        .map(|m| header_h + pad + (m.fields.len() as u32) * row_h + pad)
        .collect();

    let num_rows = model_count.div_ceil(cols);
    let mut row_max_h: Vec<u32> = vec![0; num_rows];
    for (pos, &model_idx) in ordered.iter().enumerate() {
        let row = pos / cols;
        row_max_h[row] = row_max_h[row].max(card_heights[model_idx]);
    }

    let mut row_y: Vec<u32> = vec![40; num_rows];
    for r in 1..num_rows {
        row_y[r] = row_y[r - 1] + row_max_h[r - 1] + spacing_y;
    }

    let total_w = 40 + (cols as u32) * (card_w + spacing_x);
    let total_h = if num_rows > 0 {
        row_y[num_rows - 1] + row_max_h[num_rows - 1] + 80
    } else {
        800
    };

    xml.push_str("  <diagram id=\"db\" name=\"DB Models\">\n");
    xml.push_str(&format!(
        "    <mxGraphModel dx=\"1422\" dy=\"762\" grid=\"1\" gridSize=\"10\" guides=\"1\" tooltips=\"1\" connect=\"1\" arrows=\"1\" fold=\"1\" page=\"1\" pageScale=\"1\" pageWidth=\"{}\" pageHeight=\"{}\">\n",
        total_w.max(2400), total_h.max(1600)
    ));
    xml.push_str("      <root>\n");
    xml.push_str("        <mxCell id=\"0\"/>\n");
    xml.push_str("        <mxCell id=\"1\" parent=\"0\"/>\n");

    let mut model_cell_ids: Vec<u32> = vec![0; model_count];

    for (pos, &model_idx) in ordered.iter().enumerate() {
        let model = &all_models[model_idx];
        let col = pos % cols;
        let row = pos / cols;
        let card_h = card_heights[model_idx];
        let x = 40 + (col as u32) * (card_w + spacing_x);
        let y = row_y[row];

        let group_id = ids.next();
        model_cell_ids[model_idx] = group_id;

        // Table container
        xml.push_str(&format!(
            "        <mxCell id=\"{}\" value=\"{}\" style=\"swimlane;startSize={};fillColor=#dae8fc;strokeColor=#6c8ebf;fontStyle=1;fontSize=13;rounded=1;collapsible=0;\" vertex=\"1\" parent=\"1\">\n",
            group_id, esc(&model.name), header_h
        ));
        xml.push_str(&format!(
            "          <mxGeometry x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" as=\"geometry\"/>\n",
            x, y, card_w, card_h
        ));
        xml.push_str("        </mxCell>\n");

        for (fi, (field_name, field_type)) in model.fields.iter().enumerate() {
            let fid = ids.next();
            let fy = header_h + pad + (fi as u32) * row_h;

            let is_fk = is_fk_field(field_name, field_type);
            let icon = if field_type == "FK" || field_type == "M2M" {
                "🔗 "
            } else if field_name == "id" {
                "🔑 "
            } else {
                ""
            };

            let label = format!("{}{}: {}", icon, esc(field_name), field_type);

            let (fill, stroke) = if field_name == "id" {
                ("#fff2cc", "#d6b656")
            } else if is_fk {
                ("#f8cecc", "#b85450")
            } else {
                ("#ffffff", "#d6d6d6")
            };

            xml.push_str(&format!(
                "        <mxCell id=\"{}\" value=\"{}\" style=\"text;html=1;align=left;verticalAlign=middle;resizable=0;points=[];autosize=1;fillColor={};strokeColor={};rounded=1;spacingLeft=4;fontSize=11;\" vertex=\"1\" parent=\"{}\">\n",
                fid, label, fill, stroke, group_id
            ));
            xml.push_str(&format!(
                "          <mxGeometry x=\"4\" y=\"{}\" width=\"{}\" height=\"{}\" as=\"geometry\"/>\n",
                fy, card_w - 8, row_h
            ));
            xml.push_str("        </mxCell>\n");
        }
    }

    // ── FK edges ──
    for (source_idx, targets) in model_fk_targets.iter().enumerate() {
        let source_id = model_cell_ids[source_idx];
        for (_field_name, target_idx) in targets {
            let target_id = model_cell_ids[*target_idx];
            if source_id == 0 || target_id == 0 || source_id == target_id {
                continue;
            }

            let eid = ids.next();
            xml.push_str(&format!(
                "        <mxCell id=\"{}\" style=\"endArrow=ERmandOne;startArrow=ERzeroToMany;html=1;strokeWidth=1;strokeColor=#666666;curved=1;\" edge=\"1\" source=\"{}\" target=\"{}\" parent=\"1\">\n",
                eid, source_id, target_id
            ));
            xml.push_str("          <mxGeometry relative=\"1\" as=\"geometry\"/>\n");
            xml.push_str("        </mxCell>\n");
        }
    }

    xml.push_str("      </root>\n");
    xml.push_str("    </mxGraphModel>\n");
    xml.push_str("  </diagram>\n");
}

/// Order models by BFS from the most-connected node for proximity layout.
fn bfs_order(count: usize, links: &[(usize, usize)]) -> Vec<usize> {
    let mut connection_count: Vec<usize> = vec![0; count];
    for (a, b) in links {
        connection_count[*a] += 1;
        connection_count[*b] += 1;
    }

    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); count];
    for (a, b) in links {
        if !adj[*a].contains(b) {
            adj[*a].push(*b);
        }
        if !adj[*b].contains(a) {
            adj[*b].push(*a);
        }
    }

    let mut ordered: Vec<usize> = Vec::with_capacity(count);
    let mut visited = vec![false; count];

    while ordered.len() < count {
        // The while condition guarantees an unvisited node exists, but a
        // break is cheaper than a panic if that invariant ever drifts.
        let Some(start) = (0..count)
            .filter(|i| !visited[*i])
            .max_by_key(|i| connection_count[*i])
        else {
            break;
        };

        let mut queue = std::collections::VecDeque::new();
        queue.push_back(start);
        visited[start] = true;

        while let Some(node) = queue.pop_front() {
            ordered.push(node);
            let mut neighbors: Vec<usize> = adj[node]
                .iter()
                .filter(|n| !visited[**n])
                .copied()
                .collect();
            neighbors.sort_by_key(|n| std::cmp::Reverse(connection_count[*n]));
            for n in neighbors {
                if !visited[n] {
                    visited[n] = true;
                    queue.push_back(n);
                }
            }
        }
    }

    ordered
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a DbModel from string slices.
    fn model(name: &str, fields: &[(&str, &str)]) -> db_scan::DbModel {
        db_scan::DbModel {
            name: name.to_string(),
            fields: fields
                .iter()
                .map(|(n, t)| (n.to_string(), t.to_string()))
                .collect(),
        }
    }

    fn render(models: &[db_scan::DbModel], links: &[ModelLink]) -> String {
        let mut xml = String::new();
        render_db_models_page(models, links, &mut xml);
        xml
    }

    /// Return the first XML line containing `needle` (for style assertions).
    fn line_with<'a>(xml: &'a str, needle: &str) -> &'a str {
        xml.lines()
            .find(|l| l.contains(needle))
            .unwrap_or_else(|| panic!("expected a line containing {:?} in:\n{}", needle, xml))
    }

    #[test]
    fn test_empty_models_render_nothing() {
        let xml = render(&[], &[]);
        assert!(xml.is_empty(), "no models should produce no diagram page");
    }

    #[test]
    fn test_renders_model_cards_with_field_icons_and_fk_edge() {
        let models = vec![
            model("User", &[("id", "uuid"), ("name", "text")]),
            model("Post", &[("id", "uuid"), ("author_id", "FK")]),
        ];
        let links = vec![ModelLink {
            from: 1,
            to: 0,
            field: "author_id".to_string(),
        }];
        let xml = render(&models, &links);

        assert!(
            xml.contains("<diagram id=\"db\" name=\"DB Models\">"),
            "should open the DB Models diagram page"
        );
        assert!(
            xml.contains("value=\"User\"") && xml.contains("value=\"Post\""),
            "each model should get a swimlane card"
        );
        assert!(
            xml.contains("🔑 id: uuid"),
            "id fields should get the key icon"
        );
        assert!(
            xml.contains("🔗 author_id: FK"),
            "FK fields should get the link icon"
        );
        let fk_line = line_with(&xml, "author_id: FK");
        assert!(
            fk_line.contains("#f8cecc"),
            "FK fields should use the FK fill color: {}",
            fk_line
        );
        let id_line = line_with(&xml, "🔑 id: uuid");
        assert!(
            id_line.contains("#fff2cc"),
            "id fields should use the primary-key fill color: {}",
            id_line
        );
        let edge_line = line_with(&xml, "edge=\"1\"");
        assert!(
            edge_line.contains("ERmandOne") && edge_line.contains("ERzeroToMany"),
            "FK edges should use ER notation arrows: {}",
            edge_line
        );
        assert_eq!(
            xml.matches("edge=\"1\"").count(),
            1,
            "exactly one FK edge expected"
        );
    }

    #[test]
    fn test_fk_by_naming_convention_gets_fk_fill() {
        // uuid field ending in _id is treated as FK by is_fk_field.
        let models = vec![model("Session", &[("user_id", "uuid")])];
        let xml = render(&models, &[]);

        let line = line_with(&xml, "user_id: uuid");
        assert!(
            line.contains("#f8cecc"),
            "uuid *_id fields should be styled as FKs: {}",
            line
        );
    }

    #[test]
    fn test_self_referential_link_produces_no_edge() {
        let models = vec![model("Node", &[("id", "uuid"), ("parent_id", "FK")])];
        let links = vec![ModelLink {
            from: 0,
            to: 0,
            field: "parent_id".to_string(),
        }];
        let xml = render(&models, &links);

        assert_eq!(
            xml.matches("edge=\"1\"").count(),
            0,
            "self-referential FK edges must be skipped"
        );
    }

    #[test]
    fn test_escapes_special_chars_in_model_and_field_names() {
        let models = vec![model("Order<T> & Co", &[("a&b", "text")])];
        let xml = render(&models, &[]);

        assert!(
            xml.contains("value=\"Order&lt;T&gt; &amp; Co\""),
            "model names must be XML-escaped"
        );
        assert!(
            xml.contains("a&amp;b: text"),
            "field names must be XML-escaped"
        );
        assert!(
            !xml.contains("value=\"Order<T>"),
            "raw unescaped model name must not appear"
        );
    }

    #[test]
    fn test_bfs_order_starts_with_most_connected_node() {
        // Node 1 has three connections; it should lead the layout order.
        let ordered = bfs_order(4, &[(0, 1), (1, 2), (1, 3)]);
        assert_eq!(ordered[0], 1, "most-connected node should come first");
        let mut sorted = ordered.clone();
        sorted.sort_unstable();
        assert_eq!(
            sorted,
            vec![0, 1, 2, 3],
            "all nodes must be ordered exactly once"
        );
    }

    #[test]
    fn test_bfs_order_includes_isolated_nodes() {
        let ordered = bfs_order(3, &[(0, 1)]);
        assert_eq!(ordered.len(), 3, "isolated nodes must still be placed");
        assert!(
            ordered.contains(&2),
            "node with no links should appear in the order"
        );
    }
}
