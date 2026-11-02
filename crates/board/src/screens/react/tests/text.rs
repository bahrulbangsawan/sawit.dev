//! Text, whitespace, comments and elements as JSX.

use super::jsx;

#[test]
fn void_and_empty_elements_self_close() {
    let out = jsx(
        r#"<p>a<br>b</p><hr><img src="https://x.example/a.png" alt=""><input><div></div><span></span>"#,
    );
    assert_eq!(
        out,
        "<p>\n  a\n  <br />\n  b\n</p>\n<hr />\n<img src=\"https://x.example/a.png\" alt=\"\" />\n\
         <input />\n<div />\n<span />"
    );
}

#[test]
fn comments_become_jsx_comments() {
    let out = jsx("<div><!-- a */ b --><p>x</p></div><!---->");
    assert_eq!(
        out,
        "<div>\n  {/* a * / b */}\n  <p>x</p>\n</div>\n{/*  */}"
    );
}

#[test]
fn entities_and_jsx_characters_are_escaped() {
    let out = jsx("<p>Tom &amp; Jerry &lt;3 &copy; {braces} a&nbsp;b &gt; it's \"q\"</p>");
    assert_eq!(
        out,
        "<p>Tom &amp; Jerry {\"<\"}3 \u{a9} {\"{\"}braces{\"}\"} a&nbsp;b {\">\"} it's \"q\"</p>"
    );
}

#[test]
fn whitespace_shows_as_html_renders_it() {
    let out = jsx("
  <div>
    <p>Hello <b>world</b>   and
       <i>you</i></p>
    <span>a</span> <span>b</span>
    <p>   lots   of

     spaces   </p>
    <span>c</span><br>
    d
  </div>
  tail
");
    let expected = r#"<div>
  <p>
    Hello{" "}
    <b>world</b>
    {" "}and{" "}
    <i>you</i>
  </p>
  <span>a</span>
  {" "}
  <span>b</span>
  <p>lots of spaces</p>
  <span>c</span>
  <br />
  d
</div>
tail"#;
    assert_eq!(out, expected);
}

#[test]
fn spaces_at_an_inline_elements_edges_stay() {
    let out = jsx("<p>a<em> b </em>c</p>");
    assert_eq!(out, "<p>\n  a\n  <em>{\" \"}b{\" \"}</em>\n  c\n</p>");
}

#[test]
fn whitespace_goes_where_text_never_shows() {
    let out = jsx(
        "<ul>\n  <li>one</li>\n  <li>two</li>\n</ul>\n<select>\n <option>a</option>\n</select>",
    );
    assert_eq!(
        out,
        "<ul>\n  <li>one</li>\n  <li>two</li>\n</ul>\n<select>\n  <option>a</option>\n</select>"
    );
}

#[test]
fn pre_keeps_its_whitespace_exactly() {
    let out = jsx("<pre>\n  line 1\n    line 2\n<code>x  {y}</code>\n</pre><pre>one</pre>");
    assert_eq!(
        out,
        "<pre>\n  {\"  line 1\\n    line 2\\n\"}\n  <code>{\"x  {y}\"}</code>\n  {\"\\n\"}\n</pre>\n\
         <pre>{\"one\"}</pre>"
    );
}

#[test]
fn template_content_is_converted() {
    let out = jsx("<template><p>in</p></template>");
    assert_eq!(out, "<template>\n  <p>in</p>\n</template>");
}

#[test]
fn a_tag_jsx_cant_write_leaves_its_children() {
    let out = jsx("<p>a<o:p>b</o:p> c</p>");
    assert_eq!(out, "<p>ab c</p>");
}

#[test]
fn a_buttons_edges_are_trimmed_like_a_blocks() {
    let out = jsx("<button>\n  Save\n</button> <button> Go </button>");
    assert_eq!(out, "<button>Save</button>\n{\" \"}\n<button>Go</button>");
}
