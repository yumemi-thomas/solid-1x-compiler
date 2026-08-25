#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum DomPropertyState {
    Stateful,
    Locked,
}

pub(crate) fn child_properties(name: &str) -> bool {
    matches!(name, "innerHTML" | "textContent" | "innerText" | "children")
}

pub(crate) fn boolean_attributes(name: &str) -> bool {
    matches!(
        name,
        "allowfullscreen"
            | "async"
            | "alpha"
            | "autofocus"
            | "autoplay"
            | "checked"
            | "controls"
            | "default"
            | "disabled"
            | "formnovalidate"
            | "hidden"
            | "indeterminate"
            | "inert"
            | "ismap"
            | "loop"
            | "multiple"
            | "muted"
            | "nomodule"
            | "novalidate"
            | "open"
            | "playsinline"
            | "readonly"
            | "required"
            | "reversed"
            | "seamless"
            | "selected"
            | "adauctionheaders"
            | "browsingtopics"
            | "credentialless"
            | "defaultchecked"
            | "defaultmuted"
            | "defaultselected"
            | "defer"
            | "disablepictureinpicture"
            | "disableremoteplayback"
            | "preservespitch"
            | "shadowrootclonable"
            | "shadowrootcustomelementregistry"
            | "shadowrootdelegatesfocus"
            | "shadowrootserializable"
            | "sharedstoragewritable"
    )
}

pub(crate) fn dom_properties(name: &str) -> bool {
    boolean_attributes(name)
        || matches!(
            name,
            "className"
                | "value"
                | "readOnly"
                | "noValidate"
                | "formNoValidate"
                | "isMap"
                | "noModule"
                | "playsInline"
                | "adAuctionHeaders"
                | "allowFullscreen"
                | "browsingTopics"
                | "defaultChecked"
                | "defaultMuted"
                | "defaultSelected"
                | "disablePictureInPicture"
                | "disableRemotePlayback"
                | "preservesPitch"
                | "shadowRootClonable"
                | "shadowRootCustomElementRegistry"
                | "shadowRootDelegatesFocus"
                | "shadowRootSerializable"
                | "sharedStorageWritable"
        )
}

pub(crate) fn prop_alias(name: &str, tag_name: &str) -> Option<&'static str> {
    let tag = tag_name.to_ascii_uppercase();
    match name {
        "class" => Some("className"),
        "novalidate" if tag == "FORM" => Some("noValidate"),
        "formnovalidate" if matches!(tag.as_str(), "BUTTON" | "INPUT") => Some("formNoValidate"),
        "ismap" if tag == "IMG" => Some("isMap"),
        "nomodule" if tag == "SCRIPT" => Some("noModule"),
        "playsinline" if tag == "VIDEO" => Some("playsInline"),
        "readonly" if matches!(tag.as_str(), "INPUT" | "TEXTAREA") => Some("readOnly"),
        "adauctionheaders" if tag == "IFRAME" => Some("adAuctionHeaders"),
        "allowfullscreen" if tag == "IFRAME" => Some("allowFullscreen"),
        "browsingtopics" if tag == "IMG" => Some("browsingTopics"),
        "defaultchecked" if tag == "INPUT" => Some("defaultChecked"),
        "defaultmuted" if matches!(tag.as_str(), "AUDIO" | "VIDEO") => Some("defaultMuted"),
        "defaultselected" if tag == "OPTION" => Some("defaultSelected"),
        "disablepictureinpicture" if tag == "VIDEO" => Some("disablePictureInPicture"),
        "disableremoteplayback" if matches!(tag.as_str(), "AUDIO" | "VIDEO") => {
            Some("disableRemotePlayback")
        }
        "preservespitch" if matches!(tag.as_str(), "AUDIO" | "VIDEO") => Some("preservesPitch"),
        "shadowrootclonable" if tag == "TEMPLATE" => Some("shadowRootClonable"),
        "shadowrootdelegatesfocus" if tag == "TEMPLATE" => Some("shadowRootDelegatesFocus"),
        "shadowrootserializable" if tag == "TEMPLATE" => Some("shadowRootSerializable"),
        "sharedstoragewritable" if matches!(tag.as_str(), "IFRAME" | "IMG") => {
            Some("sharedStorageWritable")
        }
        _ => None,
    }
}

pub(crate) fn delegated_events(name: &str) -> bool {
    matches!(
        name,
        "beforeinput"
            | "click"
            | "dblclick"
            | "contextmenu"
            | "focusin"
            | "focusout"
            | "input"
            | "keydown"
            | "keyup"
            | "mousedown"
            | "mousemove"
            | "mouseout"
            | "mouseover"
            | "mouseup"
            | "pointerdown"
            | "pointermove"
            | "pointerout"
            | "pointerover"
            | "pointerup"
            | "touchend"
            | "touchmove"
            | "touchstart"
    )
}

pub(crate) const ALWAYS_CLOSE_ELEMENTS: &[&str] = &[
    "title", "style", "a", "strong", "small", "b", "u", "i", "em", "s", "code", "object", "table",
    "button", "textarea", "select", "iframe", "script", "noscript", "template", "fieldset",
];

pub(crate) const BLOCK_ELEMENTS: &[&str] = &[
    "address",
    "article",
    "aside",
    "blockquote",
    "dd",
    "details",
    "dialog",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hgroup",
    "hr",
    "li",
    "main",
    "menu",
    "nav",
    "ol",
    "p",
    "pre",
    "section",
    "table",
    "ul",
];

pub(crate) const INLINE_ELEMENTS: &[&str] = &[
    "a", "abbr", "acronym", "b", "bdi", "bdo", "big", "br", "button", "canvas", "cite", "code",
    "data", "datalist", "del", "dfn", "em", "embed", "i", "iframe", "img", "input", "ins", "kbd",
    "label", "map", "mark", "meter", "noscript", "object", "output", "picture", "progress", "q",
    "ruby", "s", "samp", "script", "select", "slot", "small", "span", "strong", "sub", "sup",
    "svg", "template", "textarea", "time", "u", "tt", "var", "video",
];

pub(crate) fn inline_elements(name: &str) -> bool {
    INLINE_ELEMENTS.contains(&name)
}

/// SVG element names, mirroring the runtime's `SVGElements` set (`a`,
/// `script`, `style`, and `title` excluded as HTML-ambiguous).
pub(crate) const SVG_ELEMENTS: &[&str] = &[
    "altGlyph",
    "altGlyphDef",
    "altGlyphItem",
    "animate",
    "animateColor",
    "animateMotion",
    "animateTransform",
    "circle",
    "clipPath",
    "color-profile",
    "cursor",
    "defs",
    "desc",
    "ellipse",
    "feBlend",
    "feColorMatrix",
    "feComponentTransfer",
    "feComposite",
    "feConvolveMatrix",
    "feDiffuseLighting",
    "feDisplacementMap",
    "feDistantLight",
    "feDropShadow",
    "feFlood",
    "feFuncA",
    "feFuncB",
    "feFuncG",
    "feFuncR",
    "feGaussianBlur",
    "feImage",
    "feMerge",
    "feMergeNode",
    "feMorphology",
    "feOffset",
    "fePointLight",
    "feSpecularLighting",
    "feSpotLight",
    "feTile",
    "feTurbulence",
    "filter",
    "font",
    "font-face",
    "font-face-format",
    "font-face-name",
    "font-face-src",
    "font-face-uri",
    "foreignObject",
    "g",
    "glyph",
    "glyphRef",
    "hkern",
    "image",
    "line",
    "linearGradient",
    "marker",
    "mask",
    "metadata",
    "missing-glyph",
    "mpath",
    "path",
    "pattern",
    "polygon",
    "polyline",
    "radialGradient",
    "rect",
    "set",
    "stop",
    "svg",
    "switch",
    "symbol",
    "text",
    "textPath",
    "tref",
    "tspan",
    "use",
    "view",
    "vkern",
];

pub(crate) fn svg_elements(name: &str) -> bool {
    SVG_ELEMENTS.contains(&name)
}

/// MathML element names, mirroring the runtime's `MathMLElements` set.
pub(crate) const MATHML_ELEMENTS: &[&str] = &[
    "annotation",
    "annotation-xml",
    "maction",
    "math",
    "menclose",
    "merror",
    "mfenced",
    "mfrac",
    "mi",
    "mmultiscripts",
    "mn",
    "mo",
    "mover",
    "mpadded",
    "mphantom",
    "mprescripts",
    "mroot",
    "mrow",
    "ms",
    "mspace",
    "msqrt",
    "mstyle",
    "msub",
    "msubsup",
    "msup",
    "mtable",
    "mtd",
    "mtext",
    "mtr",
    "munder",
    "munderover",
    "semantics",
];

pub(crate) fn mathml_elements(name: &str) -> bool {
    MATHML_ELEMENTS.contains(&name)
}

pub(crate) fn dom_with_state(tag_name: &str, name: &str) -> Option<DomPropertyState> {
    match tag_name.to_ascii_uppercase().as_str() {
        "INPUT" => match name {
            "value" | "checked" => Some(DomPropertyState::Stateful),
            "defaultValue" | "defaultChecked" => Some(DomPropertyState::Locked),
            _ => None,
        },
        "SELECT" => match name {
            "value" => Some(DomPropertyState::Stateful),
            _ => None,
        },
        "OPTION" => match name {
            "value" | "selected" => Some(DomPropertyState::Stateful),
            "defaultSelected" => Some(DomPropertyState::Locked),
            _ => None,
        },
        "TEXTAREA" => match name {
            "value" => Some(DomPropertyState::Stateful),
            "defaultValue" => Some(DomPropertyState::Locked),
            _ => None,
        },
        "VIDEO" | "AUDIO" => match name {
            "muted" => Some(DomPropertyState::Stateful),
            "defaultMuted" => Some(DomPropertyState::Locked),
            _ => None,
        },
        _ => None,
    }
}

pub(crate) fn namespaces(prefix: &str) -> Option<&'static str> {
    match prefix {
        "svg" => Some("http://www.w3.org/2000/svg"),
        "mathml" => Some("http://www.w3.org/1998/Math/MathML"),
        "xlink" => Some("http://www.w3.org/1999/xlink"),
        "xml" => Some("http://www.w3.org/XML/1998/namespace"),
        _ => None,
    }
}

/// Babel main's `reservedNameSpaces`: JSX namespace prefixes with Solid 1.x
/// compiler semantics rather than XML namespace semantics.
pub(crate) fn reserved_namespace(prefix: &str) -> bool {
    matches!(
        prefix,
        "class" | "on" | "oncapture" | "style" | "use" | "prop" | "attr" | "bool"
    )
}

pub(crate) fn void_elements(tag_name: &str) -> bool {
    matches!(
        tag_name,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "keygen"
            | "link"
            | "menuitem"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}
