//! Draw.io DB models page generation with FK-proximity layout.

use super::super::db_models as db_scan;

use super::common::*;

pub(crate) fn render_db_models_page(all_models: &[db_scan::DbModel], xml: &mut String) {
    if all_models.is_empty() {
        return;
    }

    // ── Build adjacency graph for FK relationships ──
    let model_count = all_models.len();
    let model_names: Vec<String> = all_models.iter().map(|m| m.name.clone()).collect();

    let name_to_idx: std::collections::HashMap<String, usize> = model_names
        .iter()
        .enumerate()
        .map(|(i, n)| (n.to_lowercase(), i))
        .collect();

    let mut fk_links: Vec<(usize, usize)> = Vec::new();
    let mut model_fk_targets: Vec<Vec<(String, usize)>> = vec![Vec::new(); model_count];

    for (idx, model) in all_models.iter().enumerate() {
        for (field_name, field_type) in &model.fields {
            let is_fk = field_type == "FK"
                || field_type == "M2M"
                || (field_type == "uuid"
                    && (field_name.ends_with("Id") || field_name.ends_with("_id")));
            if is_fk {
                let target = field_name
                    .trim_end_matches("Id")
                    .trim_end_matches("_id")
                    .to_lowercase();
                if target.is_empty() {
                    continue;
                }
                let target_idx = name_to_idx
                    .get(&target)
                    .or_else(|| name_to_idx.get(&format!("{}s", target)))
                    .or_else(|| {
                        name_to_idx
                            .iter()
                            .find(|(k, _)| k.trim_end_matches('s') == target)
                            .map(|(_, v)| v)
                    });
                if let Some(&tidx) = target_idx
                    && tidx != idx
                {
                    fk_links.push((idx, tidx));
                    model_fk_targets[idx].push((field_name.clone(), tidx));
                }
            }
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

            let is_fk = field_type == "FK"
                || field_type == "M2M"
                || (field_type == "uuid"
                    && (field_name.ends_with("Id") || field_name.ends_with("_id")));
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
        let start = (0..count)
            .filter(|i| !visited[*i])
            .max_by_key(|i| connection_count[*i])
            .unwrap();

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
