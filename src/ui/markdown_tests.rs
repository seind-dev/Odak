use super::*;

fn para(blocks: &[Block]) -> &Inline {
    match &blocks[0] {
        Block::Paragraph(inline) => inline,
        other => panic!("expected a paragraph, got {other:?}"),
    }
}

#[test]
fn inline_styles_become_ordered_spans() {
    let blocks = parse("a **b** *c* ~~d~~ `e`");
    let p = para(&blocks);
    assert_eq!(p.text, "a b c d e");
    let styled: Vec<(&str, Style)> = p.spans.iter().map(|(r, s)| (&p.text[r.clone()], *s)).collect();
    assert_eq!(
        styled,
        vec![
            ("a ", Style::default()),
            ("b", Style { bold: true, ..Default::default() }),
            (" ", Style::default()),
            ("c", Style { italic: true, ..Default::default() }),
            (" ", Style::default()),
            ("d", Style { strike: true, ..Default::default() }),
            (" ", Style::default()),
            ("e", Style { code: true, ..Default::default() }),
        ]
    );
}

#[test]
fn links_record_their_range_and_target() {
    let blocks = parse("bak [site](https://seind.dev) lütfen");
    let p = para(&blocks);
    assert_eq!(p.links, vec![(4..8, "https://seind.dev".to_string())]);
    assert_eq!(&p.text[4..8], "site");
    assert!(p.spans.iter().any(|(r, s)| *r == (4..8) && s.link));
}

#[test]
fn headings_paragraphs_rules_and_code() {
    let blocks = parse("# Başlık\n\nmetin\n\n---\n\n```\nkod 1\nkod 2\n```");
    assert!(matches!(&blocks[0], Block::Heading(1, i) if i.text == "Başlık"));
    assert!(matches!(&blocks[1], Block::Paragraph(i) if i.text == "metin"));
    assert_eq!(blocks[2], Block::Rule);
    assert_eq!(blocks[3], Block::Code("kod 1\nkod 2".into()));
}

#[test]
fn task_lists_and_nesting() {
    let blocks = parse("- [x] bitti\n- [ ] yapılacak\n  1. alt\n");
    let Block::List { start: None, items } = &blocks[0] else { panic!("{blocks:?}") };
    assert_eq!(items[0].checked, Some(true));
    assert!(matches!(&items[0].blocks[0], Block::Paragraph(i) if i.text.trim() == "bitti"));
    assert_eq!(items[1].checked, Some(false));
    let Block::List { start: Some(1), items: nested } = &items[1].blocks[1] else { panic!("{:?}", items[1]) };
    assert!(matches!(&nested[0].blocks[0], Block::Paragraph(i) if i.text == "alt"));
}

#[test]
fn quotes_hold_blocks() {
    let blocks = parse("> alıntı\n> devam");
    let Block::Quote(inner) = &blocks[0] else { panic!("{blocks:?}") };
    assert!(matches!(&inner[0], Block::Paragraph(i) if i.text == "alıntı devam"));
}

#[test]
fn summary_is_the_first_block_without_syntax() {
    assert_eq!(summary("**Önemli:** rapor\n\nikinci paragraf"), "Önemli: rapor");
    assert_eq!(summary("- [ ] madde bir\n- iki"), "madde bir");
    assert_eq!(summary("# Başlık\nmetin"), "Başlık");
    assert_eq!(summary(""), "");
}

#[test]
fn only_web_and_mail_links_are_opened() {
    assert!(is_safe_url("https://seind.dev"));
    assert!(is_safe_url("mailto:a@b.c"));
    assert!(!is_safe_url("file:///C:/Windows"));
    assert!(!is_safe_url("javascript:alert(1)"));
}
