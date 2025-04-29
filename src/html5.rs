//! HTML5 Parser integration using html5ever.
#![cfg(feature = "html5ever")]

use std::collections::HashMap;
use std::default::Default;
use std::io::Cursor;
use std::rc::Rc;

use html5ever::driver::ParseOpts;
use html5ever::tendril::{StrTendril, TendrilSink};
// Import parse_document and related types from html5ever
use html5ever::{parse_document, QualName, namespace_url, ns}; // Removed fragment-specific imports
use markup5ever_rcdom::{Handle, NodeData, RcDom};

use crate::error::ParseError;
use crate::id::NamespaceId;
use crate::xotdata::{Node, Xot};


// Define constants for common namespace URIs used in HTML5
const HTML_NS: &str = "http://www.w3.org/1999/xhtml";
const MATHML_NS: &str = "http://www.w3.org/1998/Math/MathML";
const SVG_NS: &str = "http://www.w3.org/2000/svg";
const XLINK_NS: &str = "http://www.w3.org/1999/xlink";
const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";
const XMLNS_NS: &str = "http://www.w3.org/2000/xmlns/";

struct DomConverter {
    // Removed xot field
    namespace_ids: HashMap<StrTendril, NamespaceId>,
    // Use the pointer to the Rc container as the key
    node_map: HashMap<*const markup5ever_rcdom::Node, Node>, // Map html5ever nodes to Xot nodes
}

impl DomConverter {
    // Takes xot only to pre-add common namespaces
    fn new(xot: &mut Xot) -> Self {
        let mut namespace_ids = HashMap::new();

        let html_ns_id = xot.add_namespace(HTML_NS);
        namespace_ids.insert(StrTendril::from(HTML_NS), html_ns_id);

        let mathml_ns_id = xot.add_namespace(MATHML_NS);
        namespace_ids.insert(StrTendril::from(MATHML_NS), mathml_ns_id);

        let svg_ns_id = xot.add_namespace(SVG_NS);
        namespace_ids.insert(StrTendril::from(SVG_NS), svg_ns_id);

        let xlink_ns_id = xot.add_namespace(XLINK_NS);
        namespace_ids.insert(StrTendril::from(XLINK_NS), xlink_ns_id);

        let xml_ns_id = xot.add_namespace(XML_NS);
        namespace_ids.insert(StrTendril::from(XML_NS), xml_ns_id);

        let xmlns_ns_id = xot.add_namespace(XMLNS_NS);
        namespace_ids.insert(StrTendril::from(XMLNS_NS), xmlns_ns_id);

        DomConverter {
            // xot removed
            namespace_ids,
            node_map: HashMap::new(),
        }
    }

    // Moved namespace logic here, takes &mut Xot
    fn get_or_add_namespace_id(&mut self, xot: &mut Xot, uri: &StrTendril) -> NamespaceId {
        if uri.is_empty() {
            return xot.no_namespace();
        }
        // Check pre-cached map first
        if let Some(id) = self.namespace_ids.get(uri) {
            return *id;
        }
        // If not found, add it to xot and cache it
        let id = xot.add_namespace(uri);
        self.namespace_ids.insert(uri.clone(), id);
        id
    }


    // Takes &mut Xot as parameter now
    fn convert_handle(&mut self, xot: &mut Xot, handle: Handle, parent_xot_node: Node) {
        // Use the raw pointer to the Rc Node container as the key.
        // This is safe as long as the RcDom lives.
        // We clear the map after conversion.
        let node_ptr = Rc::as_ptr(&handle);
        if self.node_map.contains_key(&node_ptr) {
            // Avoid cycles or redundant processing
            return;
        }

        let xot_node = match handle.data {
            NodeData::Document => {
                // This should be the root call, parent is the Xot document node
                parent_xot_node
            }
            NodeData::Doctype { .. } => {
                // Xot doesn't represent doctypes explicitly in the tree
                return;
            }
            NodeData::Text { ref contents } => {
                let text_content = contents.borrow();
                // Consolidate text nodes if possible
                if let Some(last_child) = xot.last_child(parent_xot_node) { // Use xot parameter
                    if xot.is_text(last_child) { // Use xot parameter
                        // text_node itself doesn't need to be mut, only the access via text_mut
                        let text_node = xot.text_mut(last_child).unwrap(); // Use xot parameter
                        text_node.set(&format!("{}{}", text_node.get(), *text_content));
                        // Map this html5ever node to the existing Xot text node
                        self.node_map.insert(node_ptr, last_child);
                        return; // Don't create a new node
                    }
                }
                // Create a new text node
                let text_node = xot.new_text(&text_content); // Use xot parameter
                xot.append(parent_xot_node, text_node).unwrap(); // Use xot parameter
                text_node
            }
            NodeData::Comment { ref contents } => {
                let comment_node = xot.new_comment(contents); // Use xot parameter
                xot.append(parent_xot_node, comment_node).unwrap(); // Use xot parameter
                comment_node
            }
            NodeData::Element {
                ref name,
                ref attrs,
                ..
            } => {
                // Convert Atom to StrTendril for namespace lookup
                let namespace_id = self.get_or_add_namespace_id(xot, &StrTendril::from(&*name.ns)); // Use xot parameter
                let name_id = xot.add_name_ns(&name.local, namespace_id); // Use xot parameter
                let element_node = xot.new_element(name_id); // Use xot parameter
                xot.append(parent_xot_node, element_node).unwrap(); // Use xot parameter

                // Process attributes - Stage 1: Collect data and create IDs
                let mut collected_attrs = Vec::new();
                for attr in attrs.borrow().iter() {
                    // Convert Atom to StrTendril for namespace lookup
                    let attr_ns_tendril = StrTendril::from(&*attr.name.ns);
                    let attr_ns_id = self.get_or_add_namespace_id(xot, &attr_ns_tendril); // Use xot parameter
                    // html5ever uses "" for no prefix, which aligns with Xot's empty_prefix_id
                    let attr_name_id = xot.add_name_ns(&attr.name.local, attr_ns_id); // Use xot parameter
                    collected_attrs.push((attr_name_id, attr.value.to_string()));
                }

                // Process attributes - Stage 2: Add to Xot node
                if !collected_attrs.is_empty() {
                    let mut attributes = xot.attributes_mut(element_node); // Use xot parameter
                    for (name_id, value) in collected_attrs {
                        attributes.insert(name_id, value);
                    }
                }
                element_node
            }
            NodeData::ProcessingInstruction { .. } => {
                // HTML doesn't have PIs in the same way XML does. html5ever might produce them
                // for <?xml-stylesheet ...?>, but Xot's PI handling expects a target without a namespace.
                // We'll ignore them for now to avoid potential mismatches.
                // TODO: Revisit if specific PI handling is needed.
                return;
            }
        };

        // Store the mapping before processing children
        self.node_map.insert(node_ptr, xot_node);

        // Recursively convert children
        for child_handle in handle.children.borrow().iter() {
            // Pass xot down recursively
            self.convert_handle(xot, child_handle.clone(), xot_node);
        }
    }
}

/// Parses an HTML string into a Xot document node.
///
/// than in the XML parser. Namespace handling follows HTML5 rules (e.g. implicit HTML namespace).
/// It attempts to parse potentially full documents or fragments.
pub fn parse_html(xot: &mut Xot, html: &str) -> Result<Node, ParseError> {
    let mut cursor = Cursor::new(html);
    let sink = RcDom::default();
    let parse_opts = ParseOpts {
        tree_builder: html5ever::tree_builder::TreeBuilderOpts {
            scripting_enabled: false,
            iframe_srcdoc: false,
            // Keep doctype initially, html5ever handles it during parsing.
            drop_doctype: false,
            ..Default::default()
        },
        ..Default::default()
    };

    // Use parse_document. It returns the RcDom (sink) after reading.
    let sink = parse_document(sink, parse_opts)
        .from_utf8()
        .read_from(&mut cursor)
        .map_err(|e| ParseError::HtmlParse(vec![e.to_string()]))?; // Map IO error

    // We ignore html5ever parser errors for now, as it can be noisy.
    // We rely on the conversion process and subsequent checks to validate the structure.

    // Create the Xot document node
    let document_node = xot.new_document();
    let mut converter = DomConverter::new(xot);

    // Convert the entire parsed document, starting from the RcDom's document handle
    converter.convert_handle(xot, sink.document.clone(), document_node);

    // Clear the map after conversion is fully done
    converter.node_map.clear();

    // Basic validation: Check if *any* element was added under the document node.
    // This is a minimal check; specific tests should verify the actual structure.
    if xot.first_child(document_node).is_none() {
         // If the document is completely empty after conversion (ignoring comments/PIs),
         // it might indicate a severe parsing issue not caught by html5ever's error recovery.
         // Or it could genuinely be an empty input. Returning an error might be too strict.
         // For now, we allow empty results, tests should specify expected content.
         // Consider adding a more specific error if an empty result is always invalid.
    }

    Ok(document_node)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Xot; // Import Xot for testing

    #[test]
    fn test_parse_html_simple_fragment() {
        let mut xot = Xot::new();
        let html = "<html><body><h1>Simple Success</h1></body></html>";
        let root = xot.parse_html(html).expect("Failed to parse HTML fragment");

        // html5ever parser puts html elements in the HTML namespace
        let html_ns = xot.add_namespace("http://www.w3.org/1999/xhtml");
        let html_name = xot.add_name_ns("html", html_ns);
        let body_name = xot.add_name_ns("body", html_ns);
        let h1_name = xot.add_name_ns("h1", html_ns);

        let doc_el = xot.document_element(root).expect("No document element found");
        assert_eq!(xot.element(doc_el).unwrap().name(), html_name, "Document element should be <html>");

        let body_el = xot.first_child(doc_el).expect("No child found for <html> element");
        assert_eq!(xot.element(body_el).unwrap().name(), body_name, "First child should be <body>");

        let h1_el = xot.first_child(body_el).expect("No child found for <body> element");
        assert_eq!(xot.element(h1_el).unwrap().name(), h1_name, "First child should be <h1>");

        let text_node = xot.first_child(h1_el).expect("No child found for <h1> element");
        assert!(xot.is_text(text_node), "Child of <h1> should be a text node");
        assert_eq!(xot.text_str(text_node).unwrap(), "Simple Success", "Text content mismatch");
    }


    #[test]
    fn test_parse_html_with_doctype() {
        let mut xot = Xot::new();
        let html = r#"<!DOCTYPE html>
<html>
<head><title>Test</title></head>
<body><p>Hello</p></body>
</html>"#;
        let result = xot.parse_html(html);
        assert!(result.is_ok());
        let root = result.unwrap();
        let doc_el = xot.document_element(root).unwrap();
        let element = xot.element(doc_el).unwrap();
        let name = element.name();
        // HTML elements parsed by html5ever should be in the HTML namespace
        assert_eq!(xot.local_name_str(name), "html");
        assert_eq!(xot.namespace_str(xot.namespace_for_name(name)), HTML_NS);
    }
}
