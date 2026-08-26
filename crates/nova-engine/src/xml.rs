use serde::Serialize;

/// A parsed XML element: name, attributes (order-preserved), and children.
///
/// This is Nova's own minimal tree, not `roxmltree`'s borrowed one —
/// `roxmltree::Document` borrows from the source text and doesn't
/// implement `Serialize`/`Clone`, neither of which this crate's public
/// types can do without. Parsing still goes through `roxmltree` (a real
/// XML parser, not a hand-rolled one); this tree is what's left once its
/// borrowed document is walked into owned data.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct XmlElement {
    pub name: String,
    pub attributes: Vec<(String, String)>,
    pub children: Vec<XmlNode>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum XmlNode {
    Element(XmlElement),
    Text(String),
}

impl XmlElement {
    /// Serialize back to XML text. Not guaranteed byte-identical to
    /// whatever was originally parsed (whitespace/formatting isn't
    /// preserved), but an equivalent document: same elements, attributes,
    /// and text content.
    pub fn to_xml_string(&self) -> String {
        let mut out = String::new();
        write_element(self, &mut out);
        out
    }
}

/// Parse a well-formed XML document's root element into Nova's own tree.
/// Comments, processing instructions, and DOCTYPE declarations are
/// dropped — only elements, attributes, and text content are kept, which
/// is what `{{variable}}` substitution and any future path-based
/// assertion/extraction access need.
pub(crate) fn parse_xml(text: &str) -> Result<XmlElement, String> {
    let document = roxmltree::Document::parse(text).map_err(|source| source.to_string())?;
    Ok(convert(document.root_element()))
}

fn convert(node: roxmltree::Node) -> XmlElement {
    let name = node.tag_name().name().to_string();
    let attributes = node
        .attributes()
        .map(|attribute| (attribute.name().to_string(), attribute.value().to_string()))
        .collect();
    let children = node
        .children()
        .filter_map(|child| {
            if child.is_element() {
                Some(XmlNode::Element(convert(child)))
            } else if child.is_text() {
                let text = child.text().unwrap_or("");
                if text.trim().is_empty() {
                    None
                } else {
                    Some(XmlNode::Text(text.to_string()))
                }
            } else {
                None
            }
        })
        .collect();

    XmlElement {
        name,
        attributes,
        children,
    }
}

fn write_element(element: &XmlElement, out: &mut String) {
    out.push('<');
    out.push_str(&element.name);
    for (name, value) in &element.attributes {
        out.push(' ');
        out.push_str(name);
        out.push_str("=\"");
        out.push_str(&escape_attr(value));
        out.push('"');
    }

    if element.children.is_empty() {
        out.push_str("/>");
        return;
    }

    out.push('>');
    for child in &element.children {
        match child {
            XmlNode::Element(child) => write_element(child, out),
            XmlNode::Text(text) => out.push_str(&escape_text(text)),
        }
    }
    out.push_str("</");
    out.push_str(&element.name);
    out.push('>');
}

fn escape_text(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_attr(text: &str) -> String {
    escape_text(text).replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_simple_element_with_attributes_and_text() {
        let xml = r#"<user id="42"><name>John</name></user>"#;

        let element = parse_xml(xml).unwrap();

        assert_eq!(element.name, "user");
        assert_eq!(
            element.attributes,
            vec![("id".to_string(), "42".to_string())]
        );
        assert_eq!(element.children.len(), 1);
        let XmlNode::Element(name_element) = &element.children[0] else {
            panic!("expected an element child");
        };
        assert_eq!(name_element.name, "name");
        assert_eq!(
            name_element.children,
            vec![XmlNode::Text("John".to_string())]
        );
    }

    #[test]
    fn malformed_xml_is_a_typed_error() {
        let err = parse_xml("<user><name>John</user>").unwrap_err();
        assert!(!err.is_empty());
    }

    #[test]
    fn round_trips_an_equivalent_document() {
        let xml = r#"<note importance="high"><to>Jane</to><body>Hello &amp; welcome</body></note>"#;

        let element = parse_xml(xml).unwrap();
        let serialized = element.to_xml_string();
        let reparsed = parse_xml(&serialized).unwrap();

        assert_eq!(element, reparsed);
    }

    #[test]
    fn escapes_special_characters_on_write() {
        let element = XmlElement {
            name: "note".to_string(),
            attributes: vec![(
                "label".to_string(),
                "a \"quoted\" & <tricky> value".to_string(),
            )],
            children: vec![XmlNode::Text("<hello> & \"world\"".to_string())],
        };

        let xml = element.to_xml_string();
        let reparsed = parse_xml(&xml).unwrap();

        assert_eq!(element, reparsed);
    }
}
