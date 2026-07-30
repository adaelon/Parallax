use eam_markdown::{
    MarkdownBlockKind, MarkdownLocator, MarkdownParseError, MarkdownRelationKind, ParseLimits,
    ParseResource, parse_markdown,
};

const FULL: &str = include_str!("fixtures/full-dialect.md");
const UNKNOWN: &str = include_str!("fixtures/unknown-syntax.md");
const LIMITS: &str = include_str!("fixtures/limits.md");

fn limits(
    source: usize,
    blocks: usize,
    depth: usize,
    metadata: usize,
    links: usize,
) -> ParseLimits {
    ParseLimits::new(source, blocks, depth, metadata, links).unwrap()
}

#[test]
fn full_dialect_has_stable_structure_and_verbatim_ranges() {
    let parsed = parse_markdown(FULL, ParseLimits::default()).unwrap();

    assert_eq!(parsed.contract_version, "eam-markdown-v1");
    assert_eq!(
        parsed
            .properties
            .iter()
            .map(|property| property.name.as_str())
            .collect::<Vec<_>>(),
        vec!["title", "tags", "aliases", "rating", "active"]
    );
    assert!(parsed.blocks.iter().any(|block| {
        block.kind == MarkdownBlockKind::Heading
            && block.heading_level == Some(1)
            && block.native_locator
                == Some(MarkdownLocator::Heading {
                    text: "标题 😀".to_owned(),
                })
    }));
    assert!(parsed.blocks.iter().any(|block| {
        block.kind == MarkdownBlockKind::ListItem && block.task_checked == Some(true)
    }));
    assert!(parsed.blocks.iter().any(|block| {
        block.kind == MarkdownBlockKind::ListItem && block.task_checked == Some(false)
    }));
    for expected in [
        MarkdownBlockKind::Paragraph,
        MarkdownBlockKind::BlockQuote,
        MarkdownBlockKind::CodeBlock,
        MarkdownBlockKind::Table,
        MarkdownBlockKind::TableCell,
        MarkdownBlockKind::HtmlBlock,
        MarkdownBlockKind::MetadataBlock,
    ] {
        assert!(
            parsed.blocks.iter().any(|block| block.kind == expected),
            "missing {expected:?}"
        );
    }
    assert!(parsed.blocks.iter().any(|block| {
        block.native_locator
            == Some(MarkdownLocator::BlockId {
                id: "paragraph-id".to_owned(),
            })
    }));
    assert!(parsed.blocks.iter().any(|block| {
        block.kind == MarkdownBlockKind::Table
            && block.native_locator
                == Some(MarkdownLocator::BlockId {
                    id: "table-anchor".to_owned(),
                })
    }));

    let relation_kinds = parsed
        .relations
        .iter()
        .map(|relation| relation.kind)
        .collect::<Vec<_>>();
    for expected in [
        MarkdownRelationKind::Link,
        MarkdownRelationKind::Image,
        MarkdownRelationKind::Autolink,
        MarkdownRelationKind::Wikilink,
        MarkdownRelationKind::Embed,
    ] {
        assert!(relation_kinds.contains(&expected), "missing {expected:?}");
    }
    let wiki = parsed
        .relations
        .iter()
        .find(|relation| relation.kind == MarkdownRelationKind::Wikilink)
        .unwrap();
    assert_eq!(wiki.target, "Target note");
    assert_eq!(wiki.alias.as_deref(), Some("Alias"));
    assert_eq!(wiki.heading.as_deref(), Some("Section"));
    assert_eq!(wiki.block_id, None);
    assert!(parsed.tags.iter().any(|tag| tag.value == "project/demo"));
    assert!(parsed.tags.iter().any(|tag| tag.value == "rust"));

    let spans = parsed
        .properties
        .iter()
        .map(|property| property.source_span)
        .chain(parsed.blocks.iter().map(|block| block.source_span))
        .chain(parsed.relations.iter().map(|relation| relation.source_span))
        .chain(parsed.tags.iter().map(|tag| tag.source_span));
    for span in spans {
        assert!(!span.slice(FULL).unwrap().is_empty());
    }
    let snapshot = serde_json::json!({
        "properties": parsed.properties.iter().map(|property| (
            &property.name,
            &property.values,
            property.source_span.start_byte,
            property.source_span.end_byte,
        )).collect::<Vec<_>>(),
        "blocks": parsed.blocks.iter().map(|block| (
            block.kind,
            block.parent_local_id,
            block.source_span.start_byte,
            block.source_span.end_byte,
            block.heading_level,
            block.list_start,
            block.task_checked,
            &block.info_string,
            &block.native_locator,
        )).collect::<Vec<_>>(),
        "relations": parsed.relations.iter().map(|relation| (
            relation.kind,
            &relation.target,
            &relation.alias,
            &relation.heading,
            &relation.block_id,
            relation.source_span.start_byte,
            relation.source_span.end_byte,
        )).collect::<Vec<_>>(),
        "tags": parsed.tags.iter().map(|tag| (
            &tag.value,
            tag.source_span.start_byte,
            tag.source_span.end_byte,
        )).collect::<Vec<_>>(),
    });
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/full-dialect.expected.json")).unwrap();
    assert_eq!(snapshot, expected);
}

#[test]
fn unknown_extensions_remain_source_without_specialized_semantics() {
    let parsed = parse_markdown(UNKNOWN, ParseLimits::default()).unwrap();

    assert!(parsed.properties.is_empty());
    assert!(parsed.relations.is_empty());
    assert!(parsed.tags.is_empty());
    assert!(parsed.blocks.iter().any(|block| {
        block.kind == MarkdownBlockKind::CodeBlock
            && block.info_string.as_deref() == Some("dataview")
            && block
                .source_span
                .slice(UNKNOWN)
                .unwrap()
                .contains("TABLE file.name")
    }));
    assert!(parsed.blocks.iter().any(|block| {
        block
            .source_span
            .slice(UNKNOWN)
            .unwrap()
            .contains("==highlight==")
    }));
}

#[test]
fn each_resource_limit_rejects_atomically_with_a_stable_reason() {
    let cases = [
        (
            limits(LIMITS.len() - 1, 100, 64, 1024, 100),
            ParseResource::SourceBytes,
        ),
        (
            limits(LIMITS.len(), 0, 64, 1024, 100),
            ParseResource::Blocks,
        ),
        (
            limits(LIMITS.len(), 100, 1, 1024, 100),
            ParseResource::NestingDepth,
        ),
        (
            limits(LIMITS.len(), 100, 64, 1, 100),
            ParseResource::MetadataBytes,
        ),
        (limits(LIMITS.len(), 100, 64, 1024, 0), ParseResource::Links),
    ];
    for (limits, resource) in cases {
        assert_eq!(
            parse_markdown(LIMITS, limits),
            Err(MarkdownParseError::ResourceLimit(resource))
        );
    }
}

#[test]
fn code_and_link_text_do_not_create_nested_extensions() {
    let source = "`[[code]] #code` [#label](target) [[note|#alias]]";
    let parsed = parse_markdown(source, ParseLimits::default()).unwrap();

    assert_eq!(parsed.tags.len(), 0);
    assert_eq!(
        parsed
            .relations
            .iter()
            .filter(|relation| relation.kind == MarkdownRelationKind::Wikilink)
            .count(),
        1
    );
}
