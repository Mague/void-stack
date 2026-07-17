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

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a Route directly (constructor is private to the scanner).
    fn route(
        method: &str,
        path: &str,
        handler: &str,
        summary: Option<&str>,
        internal: bool,
    ) -> Route {
        Route {
            method: method.to_string(),
            path: path.to_string(),
            handler: handler.to_string(),
            tag: None,
            summary: summary.map(String::from),
            internal,
        }
    }

    fn render(all_routes: &[(String, Vec<Route>)]) -> String {
        let mut xml = String::new();
        render_api_routes_page(all_routes, &mut xml);
        xml
    }

    /// Return the first XML line containing `needle` (for style assertions).
    fn line_with<'a>(xml: &'a str, needle: &str) -> &'a str {
        xml.lines()
            .find(|l| l.contains(needle))
            .unwrap_or_else(|| panic!("expected a line containing {:?} in:\n{}", needle, xml))
    }

    #[test]
    fn test_no_routes_renders_nothing() {
        let xml = render(&[("api".to_string(), Vec::new())]);
        assert!(
            xml.is_empty(),
            "services without routes should produce no diagram page"
        );
    }

    #[test]
    fn test_public_and_internal_routes_split_into_groups() {
        let routes = vec![
            route("GET", "/users", "list_users", None, false),
            route("POST", "/internal/sync", "sync", None, true),
        ];
        let xml = render(&[("api".to_string(), routes)]);

        assert!(
            xml.contains("<diagram id=\"api\" name=\"API Routes\">"),
            "should open the API Routes diagram page"
        );
        assert!(
            xml.contains("value=\"api\""),
            "public routes should live in a group titled after the service"
        );
        assert!(
            xml.contains("value=\"api — Internal API\""),
            "internal routes should get their own group"
        );
        let internal_group = line_with(&xml, "api — Internal API");
        assert!(
            internal_group.contains("swimlane"),
            "internal group should be a swimlane: {}",
            internal_group
        );
        assert!(
            xml.contains("GET /users"),
            "public route should be rendered"
        );
        assert!(
            xml.contains("POST /internal/sync"),
            "internal route should be rendered"
        );
    }

    #[test]
    fn test_method_color_mapping() {
        let routes = vec![
            route("GET", "/a", "h", None, false),
            route("DELETE", "/b", "h", None, false),
            route("BREW", "/c", "h", None, false), // unknown method
        ];
        let xml = render(&[("svc".to_string(), routes)]);

        let get_line = line_with(&xml, "GET /a");
        assert!(
            get_line.contains("#d5e8d4"),
            "GET routes should be green: {}",
            get_line
        );
        let del_line = line_with(&xml, "DELETE /b");
        assert!(
            del_line.contains("#f8cecc"),
            "DELETE routes should be red: {}",
            del_line
        );
        let other_line = line_with(&xml, "BREW /c");
        assert!(
            other_line.contains("#f5f5f5"),
            "unknown methods should fall back to gray: {}",
            other_line
        );
    }

    #[test]
    fn test_label_prefers_summary_over_handler() {
        let routes = vec![route(
            "GET",
            "/users",
            "list_users_handler",
            Some("List users"),
            false,
        )];
        let xml = render(&[("svc".to_string(), routes)]);

        assert!(
            xml.contains("value=\"GET /users — List users\""),
            "documented summary should win over the handler name: {}",
            xml
        );
    }

    #[test]
    fn test_label_falls_back_to_handler_or_bare_route() {
        let routes = vec![
            route("GET", "/users", "list_users", None, false),
            route("POST", "/ping", "", None, false),
        ];
        let xml = render(&[("svc".to_string(), routes)]);

        assert!(
            xml.contains("value=\"GET /users — list_users\""),
            "handler name should be used when there is no summary"
        );
        assert!(
            xml.contains("value=\"POST /ping\""),
            "empty detail should render the bare method and path without a dash"
        );
    }

    #[test]
    fn test_escapes_special_chars_in_title_path_and_summary() {
        let routes = vec![route(
            "GET",
            "/a?x=<1>&y=2",
            "h",
            Some("fetch <a> & b"),
            false,
        )];
        let xml = render(&[("svc & co".to_string(), routes)]);

        assert!(
            xml.contains("value=\"svc &amp; co\""),
            "group title must be XML-escaped"
        );
        assert!(
            xml.contains("/a?x=&lt;1&gt;&amp;y=2"),
            "route path must be XML-escaped"
        );
        assert!(
            xml.contains("fetch &lt;a&gt; &amp; b"),
            "route summary must be XML-escaped"
        );
        assert!(
            !xml.contains("x=<1>"),
            "raw unescaped path must not appear in attribute values"
        );
    }
}
