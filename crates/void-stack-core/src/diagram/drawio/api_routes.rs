//! Draw.io API routes page generation.

use super::super::api_routes::Route;

use super::common::*;

pub(crate) fn render_api_routes_page(all_routes: &[(String, Vec<Route>)], xml: &mut String) {
    if all_routes.iter().all(|(_, r)| r.is_empty()) {
        return;
    }

    let mut ids = IdGen::new();

    xml.push_str("  <diagram id=\"api\" name=\"API Routes\">\n");
    xml.push_str("    <mxGraphModel dx=\"1422\" dy=\"762\" grid=\"1\" gridSize=\"10\" guides=\"1\" tooltips=\"1\" connect=\"1\" arrows=\"1\" fold=\"1\" page=\"1\" pageScale=\"1\" pageWidth=\"1600\" pageHeight=\"1200\">\n");
    xml.push_str("      <root>\n");
    xml.push_str("        <mxCell id=\"0\"/>\n");
    xml.push_str("        <mxCell id=\"1\" parent=\"0\"/>\n");

    let mut group_x: u32 = 40;

    for (svc_name, routes) in all_routes {
        // Same public/internal split the Mermaid renderer shows.
        let public: Vec<&Route> = routes.iter().filter(|r| !r.internal).collect();
        let internal: Vec<&Route> = routes.iter().filter(|r| r.internal).collect();

        if !public.is_empty() {
            group_x = render_route_group(xml, &mut ids, svc_name, &public, group_x);
        }
        if !internal.is_empty() {
            let title = format!("{} — Internal API", svc_name);
            group_x = render_route_group(xml, &mut ids, &title, &internal, group_x);
        }
    }

    xml.push_str("      </root>\n");
    xml.push_str("    </mxGraphModel>\n");
    xml.push_str("  </diagram>\n");
}

fn render_route_group(
    xml: &mut String,
    ids: &mut IdGen,
    title: &str,
    routes: &[&Route],
    group_x: u32,
) -> u32 {
    let group_id = ids.next();
    let route_h: u32 = 36;
    let route_spacing: u32 = 6;
    let route_w: u32 = 280;
    let header_h: u32 = 30;
    let pad: u32 = 15;
    let group_h = header_h + pad + (routes.len() as u32) * (route_h + route_spacing) + pad;
    let group_w = route_w + 2 * pad;

    xml.push_str(&format!(
        "        <mxCell id=\"{}\" value=\"{}\" style=\"swimlane;startSize={};fillColor=#dae8fc;strokeColor=#6c8ebf;fontStyle=1;fontSize=13;rounded=1;\" vertex=\"1\" parent=\"1\">\n",
        group_id,
        esc(title),
        header_h
    ));
    xml.push_str(&format!(
        "          <mxGeometry x=\"{}\" y=\"40\" width=\"{}\" height=\"{}\" as=\"geometry\"/>\n",
        group_x, group_w, group_h
    ));
    xml.push_str("        </mxCell>\n");

    for (i, route) in routes.iter().enumerate() {
        let rid = ids.next();
        let y = header_h + pad + (i as u32) * (route_h + route_spacing);

        let (fill, stroke) = match route.method.as_str() {
            "GET" => ("#d5e8d4", "#82b366"),
            "POST" => ("#fff2cc", "#d6b656"),
            "PUT" => ("#dae8fc", "#6c8ebf"),
            "DELETE" => ("#f8cecc", "#b85450"),
            "PATCH" => ("#e1d5e7", "#9673a6"),
            "WS" => ("#ffe6cc", "#d79b00"),
            "RPC" | "STREAM" => ("#e1d5e7", "#9673a6"),
            _ => ("#f5f5f5", "#666666"),
        };

        // Summary when documented, handler as fallback — same as Mermaid.
        let detail = route.summary.as_deref().unwrap_or(&route.handler);
        let label = if detail.is_empty() {
            format!("{} {}", route.method, esc(&route.path))
        } else {
            format!("{} {} — {}", route.method, esc(&route.path), esc(detail))
        };

        xml.push_str(&format!(
            "        <mxCell id=\"{}\" value=\"{}\" style=\"rounded=1;whiteSpace=wrap;html=1;fillColor={};strokeColor={};fontColor=#333333;fontSize=11;align=left;spacingLeft=8;fontStyle=1;\" vertex=\"1\" parent=\"{}\">\n",
            rid, label, fill, stroke, group_id
        ));
        xml.push_str(&format!(
            "          <mxGeometry x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" as=\"geometry\"/>\n",
            pad, y, route_w, route_h
        ));
        xml.push_str("        </mxCell>\n");
    }

    group_x + group_w + 30
}
