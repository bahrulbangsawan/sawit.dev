//! An element's attributes as React props: React's names (`class` → `className`, SVG's
//! `stroke-width` → `strokeWidth`), boolean attributes bare, numbers as numbers, uncontrolled
//! form values (`defaultValue`, `defaultChecked`), and event handlers left out and kept for the
//! comment at the end of the file.

use html5ever::{Attribute, QualName};
use markup5ever_rcdom::{Handle, NodeData};

use super::{Convert, js_string};
use crate::screens::INDEX;
use crate::screens::inline::{
    AssetAttr, HTML_NS, SVG_NS, asset_attr, attr, rewrite_srcset, text_of,
};

/// A prop's value as JSX writes it.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Value {
    /// `disabled`: true.
    Bare,
    /// A string: `"…"`, or `{"…"}` when JSX's attribute strings can't hold it.
    Str(String),
    /// A JavaScript expression, written in braces: `{2}`, `{{ color: "red" }}`.
    Expr(String),
}

/// A prop of an element.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Prop {
    pub name: String,
    pub value: Value,
}

/// HTML attributes whose React name differs, by the attribute's name (React DOM's own table).
const HTML_NAMES: &[(&str, &str)] = &[
    ("accept-charset", "acceptCharset"),
    ("accesskey", "accessKey"),
    ("allowfullscreen", "allowFullScreen"),
    ("allowtransparency", "allowTransparency"),
    ("autocapitalize", "autoCapitalize"),
    ("autocomplete", "autoComplete"),
    ("autocorrect", "autoCorrect"),
    ("autofocus", "autoFocus"),
    ("autoplay", "autoPlay"),
    ("autosave", "autoSave"),
    ("cellpadding", "cellPadding"),
    ("cellspacing", "cellSpacing"),
    ("charset", "charSet"),
    ("class", "className"),
    ("classid", "classID"),
    ("colspan", "colSpan"),
    ("contenteditable", "contentEditable"),
    ("contextmenu", "contextMenu"),
    ("controlslist", "controlsList"),
    ("crossorigin", "crossOrigin"),
    ("datetime", "dateTime"),
    ("disablepictureinpicture", "disablePictureInPicture"),
    ("disableremoteplayback", "disableRemotePlayback"),
    ("enctype", "encType"),
    ("enterkeyhint", "enterKeyHint"),
    ("fetchpriority", "fetchPriority"),
    ("for", "htmlFor"),
    ("formaction", "formAction"),
    ("formenctype", "formEncType"),
    ("formmethod", "formMethod"),
    ("formnovalidate", "formNoValidate"),
    ("formtarget", "formTarget"),
    ("frameborder", "frameBorder"),
    ("hreflang", "hrefLang"),
    ("http-equiv", "httpEquiv"),
    ("imagesizes", "imageSizes"),
    ("imagesrcset", "imageSrcSet"),
    ("inputmode", "inputMode"),
    ("itemid", "itemID"),
    ("itemprop", "itemProp"),
    ("itemref", "itemRef"),
    ("itemscope", "itemScope"),
    ("itemtype", "itemType"),
    ("keyparams", "keyParams"),
    ("keytype", "keyType"),
    ("marginheight", "marginHeight"),
    ("marginwidth", "marginWidth"),
    ("maxlength", "maxLength"),
    ("mediagroup", "mediaGroup"),
    ("minlength", "minLength"),
    ("nomodule", "noModule"),
    ("novalidate", "noValidate"),
    ("playsinline", "playsInline"),
    ("popovertarget", "popoverTarget"),
    ("popovertargetaction", "popoverTargetAction"),
    ("radiogroup", "radioGroup"),
    ("readonly", "readOnly"),
    ("referrerpolicy", "referrerPolicy"),
    ("rowspan", "rowSpan"),
    ("spellcheck", "spellCheck"),
    ("srcdoc", "srcDoc"),
    ("srclang", "srcLang"),
    ("srcset", "srcSet"),
    ("tabindex", "tabIndex"),
    ("usemap", "useMap"),
];

/// Boolean props: the attribute's presence is true, whatever its value.
const BOOLEANS: &[&str] = &[
    "allowFullScreen",
    "async",
    "autoFocus",
    "autoPlay",
    "checked",
    "controls",
    "default",
    "defer",
    "disabled",
    "disablePictureInPicture",
    "disableRemotePlayback",
    "formNoValidate",
    "hidden",
    "inert",
    "itemScope",
    "loop",
    "multiple",
    "muted",
    "noModule",
    "noValidate",
    "open",
    "playsInline",
    "readOnly",
    "required",
    "reversed",
    "selected",
];

/// Props that are true when the attribute is there with no value, and strings otherwise.
const EMPTY_IS_TRUE: &[&str] = &["contentEditable", "draggable", "spellCheck", "capture"];

/// Props React types as numbers.
const NUMBERS: &[&str] = &[
    "tabIndex",
    "colSpan",
    "rowSpan",
    "maxLength",
    "minLength",
    "size",
    "rows",
    "cols",
    "span",
    "start",
    "high",
    "low",
    "optimum",
    "marginHeight",
    "marginWidth",
    "border",
];

/// Attributes that take a URL, where a `javascript:` one runs code.
const URL_ATTRS: &[&str] = &["href", "src", "action", "formaction", "xlink:href"];

/// Input types whose `value` isn't what the user types, so it stays `value`.
const FIXED_VALUE_TYPES: &[&str] = &[
    "button", "checkbox", "hidden", "image", "radio", "reset", "submit",
];

impl Convert<'_> {
    /// Element `node`'s attributes as props, in order, each name once.
    pub(super) fn props(&mut self, node: &Handle) -> Vec<Prop> {
        let NodeData::Element { name, attrs, .. } = &node.data else {
            return Vec::new();
        };
        let attrs = attrs.borrow();
        let html = &*name.ns == HTML_NS;
        let tag = &*name.local;
        let mut props: Vec<Prop> = Vec::new();
        for attribute in attrs.iter() {
            let written = attr_name(&attribute.name);
            let lower = written.to_ascii_lowercase();
            let value = attribute.value.to_string();
            let scripted = (html || &*name.ns == SVG_NS) && is_handler(&lower);
            let javascript = URL_ATTRS.contains(&&*lower)
                && value
                    .trim_start()
                    .to_ascii_lowercase()
                    .starts_with("javascript:");
            if scripted || javascript {
                self.leave_out(&written, &value, tag, &attrs, scripted);
                continue;
            }
            // An HTML element's namespace is its tag's; React types no `xmlns` on one.
            if html && (lower == "xmlns" || lower.starts_with("xmlns:")) {
                continue;
            }
            if lower == "style" {
                if let Some(object) = self.style(&value) {
                    props.push(Prop {
                        name: "style".into(),
                        value: Value::Expr(object),
                    });
                }
                continue;
            }
            let value = match asset_attr(name, &attribute.name, &attrs) {
                Some(AssetAttr::Url) => self.asset(INDEX, &value).unwrap_or(value),
                Some(AssetAttr::Srcset) => {
                    rewrite_srcset(&value, |url| self.asset(INDEX, url)).unwrap_or(value)
                }
                None => value,
            };
            if html && let Some(prop) = form_value(tag, &lower, &value, &attrs) {
                props.extend(prop);
                continue;
            }
            let prop = prop_name(name, &written);
            let value = typed(&prop, value, html && !tag.contains('-'));
            props.push(Prop { name: prop, value });
        }
        if html && tag == "textarea" {
            let text = text_of(node);
            if !text.is_empty() {
                let value = Value::Expr(js_string(&text));
                props.push(Prop {
                    name: "defaultValue".into(),
                    value,
                });
            }
        }
        if html && tag == "select" {
            let multiple = attr(&attrs, "multiple").is_some();
            if let Some(value) = selected(node, multiple) {
                props.push(Prop {
                    name: "defaultValue".into(),
                    value,
                });
            }
        }
        if props.iter().any(|prop| prop.name == "contentEditable") {
            let value = Value::Bare;
            props.push(Prop {
                name: "suppressContentEditableWarning".into(),
                value,
            });
        }
        let mut seen: Vec<String> = Vec::new();
        props.retain(|prop| match seen.contains(&prop.name) {
            true => false,
            false => {
                seen.push(prop.name.clone());
                true
            }
        });
        props
    }

    /// Leaves attribute `name` (an event handler, or a `javascript:` URL) of element `tag` out,
    /// with a note, its code kept for the comment at the end.
    fn leave_out(&mut self, name: &str, code: &str, tag: &str, attrs: &[Attribute], handler: bool) {
        let element = describe(tag, attrs);
        self.note(match handler {
            true => format!(
                "{name} on {element} was left out: React takes event handlers as props; its code \
                 is at the end of this file."
            ),
            false => format!(
                "The javascript: URL in {name} on {element} was left out: React blocks them; its \
                 code is at the end of this file."
            ),
        });
        self.left_out.push(format!("{name} on {element}:\n{code}"));
    }
}

/// An attribute's name as the page wrote it: `href`, `xlink:href`, `xmlns:xlink`, `xmlns`
/// (which html5ever gives an empty prefix).
fn attr_name(name: &QualName) -> String {
    match name.prefix.as_ref().filter(|prefix| !prefix.is_empty()) {
        Some(prefix) => format!("{prefix}:{}", name.local),
        None => name.local.to_string(),
    }
}

/// Whether `name` (lowercased) is an event handler's: `on` and a word (`onclick`).
fn is_handler(name: &str) -> bool {
    name.len() > 2 && name.starts_with("on") && name.bytes().all(|b| b.is_ascii_lowercase())
}

/// The prop attribute `written` of element `element` is, by React's names: `data-*` and `aria-*`
/// as written; on HTML elements React DOM's table, and what isn't in it as written; on SVG
/// elements a name with `-` or `:` in camel case (`stroke-width` → `strokeWidth`, `xlink:href`
/// → `xlinkHref`); on custom elements and MathML only `class` changes.
pub(super) fn prop_name(element: &QualName, written: &str) -> String {
    let lower = written.to_ascii_lowercase();
    if lower.starts_with("data-") || lower.starts_with("aria-") {
        return written.to_string();
    }
    let html = &*element.ns == HTML_NS && !element.local.contains('-');
    if html && let Some((_, prop)) = HTML_NAMES.iter().find(|(name, _)| *name == lower) {
        return prop.to_string();
    }
    match &*lower {
        "class" => return "className".into(),
        "tabindex" if &*element.ns == SVG_NS => return "tabIndex".into(),
        _ => {}
    }
    if &*element.ns == SVG_NS && written.contains(['-', ':']) {
        return camel(written);
    }
    written.to_string()
}

/// `name` in camel case: each letter after a `-` or `:` uppercased, the `-` and `:` gone.
pub(super) fn camel(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut upper = false;
    for c in name.chars() {
        match c {
            '-' | ':' => upper = true,
            c if upper => {
                out.extend(c.to_uppercase());
                upper = false;
            }
            c => out.push(c),
        }
    }
    out
}

/// Attribute value `value` as prop `prop`'s: bare for a boolean, a number where React wants one
/// and it is one, else the string. `html` is false for SVG, MathML and custom elements, whose
/// attributes stay strings but for `tabIndex`.
fn typed(prop: &str, value: String, html: bool) -> Value {
    if prop == "tabIndex" && is_number(value.trim()) {
        return Value::Expr(value.trim().to_string());
    }
    if !html {
        return Value::Str(value);
    }
    if BOOLEANS.contains(&prop) && !(prop == "hidden" && value.eq_ignore_ascii_case("until-found"))
    {
        return Value::Bare;
    }
    if EMPTY_IS_TRUE.contains(&prop) && value.is_empty() {
        return Value::Bare;
    }
    if NUMBERS.contains(&prop) && is_number(value.trim()) {
        return Value::Expr(value.trim().to_string());
    }
    Value::Str(value)
}

/// Whether `text` is a plain decimal number: `2`, `-1`, `0.5`.
fn is_number(text: &str) -> bool {
    let digits = text.strip_prefix('-').unwrap_or(text);
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, "0"));
    !whole.is_empty()
        && !fraction.is_empty()
        && whole.bytes().all(|b| b.is_ascii_digit())
        && fraction.bytes().all(|b| b.is_ascii_digit())
}

/// The props an `<input>`'s `value` and `checked`, or an `<option>`'s `selected`, become: the
/// uncontrolled `defaultValue` and `defaultChecked`, and nothing for `selected` (its `<select>`
/// takes it). `None` for other attributes.
fn form_value(tag: &str, name: &str, value: &str, attrs: &[Attribute]) -> Option<Option<Prop>> {
    let kind = attr(attrs, "type")
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    match (tag, name) {
        ("input", "value") if !FIXED_VALUE_TYPES.contains(&&*kind) => Some(Some(Prop {
            name: "defaultValue".into(),
            value: Value::Str(value.to_string()),
        })),
        ("input", "checked") => Some(Some(Prop {
            name: "defaultChecked".into(),
            value: Value::Bare,
        })),
        ("option", "selected") => Some(None),
        _ => None,
    }
}

/// A `<select>`'s `defaultValue`: what its `selected` options hold (an option's `value`, else its
/// text), the last one, or all of them in an array when it is `multiple`. `None` when no option
/// is selected.
fn selected(select: &Handle, multiple: bool) -> Option<Value> {
    let mut values: Vec<String> = Vec::new();
    let mut stack: Vec<Handle> = select.children.borrow().iter().rev().cloned().collect();
    while let Some(node) = stack.pop() {
        let NodeData::Element { name, attrs, .. } = &node.data else {
            continue;
        };
        let attrs = attrs.borrow();
        if &*name.local == "option" && attr(&attrs, "selected").is_some() {
            let text = || {
                text_of(&node)
                    .split_ascii_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            values.push(attr(&attrs, "value").map_or_else(text, str::to_string));
        }
        stack.extend(node.children.borrow().iter().rev().cloned());
    }
    match multiple {
        true if values.is_empty() => None,
        true => {
            let items: Vec<String> = values.iter().map(|value| js_string(value)).collect();
            Some(Value::Expr(format!("[{}]", items.join(", "))))
        }
        false => values.pop().map(Value::Str),
    }
}

/// An element as a note names it: `<button id="save">`, `<a class="nav">`, `<div>`.
fn describe(tag: &str, attrs: &[Attribute]) -> String {
    let named = ["id", "class", "name"].iter().find_map(|key| {
        attr(attrs, key)
            .filter(|v| !v.trim().is_empty())
            .map(|v| (key, v))
    });
    match named {
        Some((key, value)) => {
            let value: String = value.trim().chars().take(40).collect();
            format!("<{tag} {key}=\"{value}\">")
        }
        None => format!("<{tag}>"),
    }
}
