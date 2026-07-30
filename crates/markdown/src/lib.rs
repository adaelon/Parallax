use std::{collections::HashSet, ops::Range};

use pulldown_cmark::{
    CodeBlockKind, Event, HeadingLevel, LinkType, MetadataBlockKind, Options, Parser, Tag, TagEnd,
};
use saphyr::{LoadableYamlNode, Scalar, Yaml};
use serde::{Deserialize, Serialize};

pub const CONTRACT_VERSION: &str = "eam-markdown-v1";
pub const MAX_SOURCE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_BLOCKS: usize = 50_000;
pub const MAX_NESTING_DEPTH: usize = 64;
pub const MAX_METADATA_BYTES: usize = 256 * 1024;
pub const MAX_LINKS: usize = 10_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParseLimits {
    max_source_bytes: usize,
    max_blocks: usize,
    max_nesting_depth: usize,
    max_metadata_bytes: usize,
    max_links: usize,
}

impl ParseLimits {
    /// Creates limits that may tighten, but never exceed, the v1 hard ceilings.
    ///
    /// # Errors
    ///
    /// Returns the first resource whose requested limit exceeds the contract.
    pub fn new(
        max_source_bytes: usize,
        max_blocks: usize,
        max_nesting_depth: usize,
        max_metadata_bytes: usize,
        max_links: usize,
    ) -> Result<Self, ParseLimitError> {
        let requested = [
            (
                ParseResource::SourceBytes,
                max_source_bytes,
                MAX_SOURCE_BYTES,
            ),
            (ParseResource::Blocks, max_blocks, MAX_BLOCKS),
            (
                ParseResource::NestingDepth,
                max_nesting_depth,
                MAX_NESTING_DEPTH,
            ),
            (
                ParseResource::MetadataBytes,
                max_metadata_bytes,
                MAX_METADATA_BYTES,
            ),
            (ParseResource::Links, max_links, MAX_LINKS),
        ];
        if let Some((resource, _, hard_maximum)) = requested
            .into_iter()
            .find(|(_, value, hard_maximum)| value > hard_maximum)
        {
            return Err(ParseLimitError {
                resource,
                hard_maximum,
            });
        }
        Ok(Self {
            max_source_bytes,
            max_blocks,
            max_nesting_depth,
            max_metadata_bytes,
            max_links,
        })
    }
}

impl Default for ParseLimits {
    fn default() -> Self {
        Self {
            max_source_bytes: MAX_SOURCE_BYTES,
            max_blocks: MAX_BLOCKS,
            max_nesting_depth: MAX_NESTING_DEPTH,
            max_metadata_bytes: MAX_METADATA_BYTES,
            max_links: MAX_LINKS,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ParseResource {
    SourceBytes,
    Blocks,
    NestingDepth,
    MetadataBytes,
    Links,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParseLimitError {
    pub resource: ParseResource,
    pub hard_maximum: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkdownParseError {
    ResourceLimit(ParseResource),
    InvalidStructure,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSpan {
    pub start_byte: usize,
    pub end_byte: usize,
}

impl SourceSpan {
    fn new(source: &str, range: Range<usize>) -> Result<Self, MarkdownParseError> {
        if range.start > range.end
            || range.end > source.len()
            || !source.is_char_boundary(range.start)
            || !source.is_char_boundary(range.end)
        {
            return Err(MarkdownParseError::InvalidStructure);
        }
        Ok(Self {
            start_byte: range.start,
            end_byte: range.end,
        })
    }

    #[must_use]
    pub fn slice<'a>(&self, source: &'a str) -> Option<&'a str> {
        source.get(self.start_byte..self.end_byte)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedMarkdownV1 {
    pub contract_version: String,
    pub properties: Vec<MarkdownProperty>,
    pub blocks: Vec<MarkdownBlock>,
    pub relations: Vec<MarkdownRelation>,
    pub tags: Vec<MarkdownTag>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkdownProperty {
    pub name: String,
    pub values: Vec<String>,
    pub source_span: SourceSpan,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkdownBlock {
    pub local_id: u64,
    pub parent_local_id: Option<u64>,
    pub ordinal: usize,
    pub kind: MarkdownBlockKind,
    pub source_span: SourceSpan,
    pub heading_level: Option<u8>,
    pub list_start: Option<u64>,
    pub task_checked: Option<bool>,
    pub info_string: Option<String>,
    pub native_locator: Option<MarkdownLocator>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MarkdownBlockKind {
    Paragraph,
    Heading,
    BlockQuote,
    List,
    ListItem,
    CodeBlock,
    Table,
    TableHead,
    TableRow,
    TableCell,
    HtmlBlock,
    ThematicBreak,
    MetadataBlock,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MarkdownLocator {
    Heading { text: String },
    BlockId { id: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkdownRelation {
    pub kind: MarkdownRelationKind,
    pub target: String,
    pub alias: Option<String>,
    pub heading: Option<String>,
    pub block_id: Option<String>,
    pub source_span: SourceSpan,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MarkdownRelationKind {
    Link,
    Image,
    Autolink,
    Wikilink,
    Embed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkdownTag {
    pub value: String,
    pub source_span: SourceSpan,
}

/// Parses one already validated UTF-8 Markdown source under immutable v1 ceilings.
///
/// # Errors
///
/// Returns a stable resource error when a caller-provided limit is exceeded, or
/// `InvalidStructure` if a parser range or stack transition violates the contract.
pub fn parse_markdown(
    source_utf8: &str,
    limits: ParseLimits,
) -> Result<ParsedMarkdownV1, MarkdownParseError> {
    if source_utf8.len() > limits.max_source_bytes {
        return Err(MarkdownParseError::ResourceLimit(
            ParseResource::SourceBytes,
        ));
    }

    let frontmatter = find_frontmatter(source_utf8)?;
    if frontmatter
        .as_ref()
        .is_some_and(|value| value.body.end - value.body.start > limits.max_metadata_bytes)
    {
        return Err(MarkdownParseError::ResourceLimit(
            ParseResource::MetadataBytes,
        ));
    }
    let (properties, tags) = frontmatter.as_ref().map_or_else(
        || (Vec::new(), Vec::new()),
        |value| parse_properties(source_utf8, value),
    );

    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_GFM);
    options.insert(Options::ENABLE_YAML_STYLE_METADATA_BLOCKS);
    options.insert(Options::ENABLE_WIKILINKS);

    let mut state = ParseState::new(source_utf8, limits, properties, tags);
    for (event, range) in Parser::new_ext(source_utf8, options).into_offset_iter() {
        state.consume(event, range)?;
    }
    state.finish()
}

struct ParseState<'a> {
    source: &'a str,
    limits: ParseLimits,
    properties: Vec<MarkdownProperty>,
    blocks: Vec<MarkdownBlock>,
    relations: Vec<MarkdownRelation>,
    tags: Vec<MarkdownTag>,
    container_depth: usize,
    open_blocks: Vec<(TagEnd, usize)>,
    heading_capture: Vec<(usize, String)>,
    link_depth: usize,
    code_depth: usize,
    html_depth: usize,
    metadata_depth: usize,
}

impl<'a> ParseState<'a> {
    fn new(
        source: &'a str,
        limits: ParseLimits,
        properties: Vec<MarkdownProperty>,
        tags: Vec<MarkdownTag>,
    ) -> Self {
        Self {
            source,
            limits,
            properties,
            blocks: Vec::new(),
            relations: Vec::new(),
            tags,
            container_depth: 0,
            open_blocks: Vec::new(),
            heading_capture: Vec::new(),
            link_depth: 0,
            code_depth: 0,
            html_depth: 0,
            metadata_depth: 0,
        }
    }

    fn consume(&mut self, event: Event<'a>, range: Range<usize>) -> Result<(), MarkdownParseError> {
        let span = SourceSpan::new(self.source, range)?;
        match event {
            Event::Start(tag) => self.start(tag, span),
            Event::End(end) => self.end(end),
            Event::Text(text) => {
                if let Some((_, captured)) = self.heading_capture.last_mut() {
                    captured.push_str(&text);
                }
                if self.text_extensions_enabled() {
                    let raw = span
                        .slice(self.source)
                        .ok_or(MarkdownParseError::InvalidStructure)?;
                    self.scan_literal_autolinks(raw, span.start_byte)?;
                    self.scan_inline_tags(raw, span.start_byte)?;
                }
                Ok(())
            }
            Event::Code(text) => {
                if let Some((_, captured)) = self.heading_capture.last_mut() {
                    captured.push_str(&text);
                }
                Ok(())
            }
            Event::Rule => self.push_standalone_block(MarkdownBlockKind::ThematicBreak, span),
            Event::TaskListMarker(checked) => {
                let item_index = self
                    .open_blocks
                    .iter()
                    .rev()
                    .find_map(|(_, index)| {
                        (self.blocks[*index].kind == MarkdownBlockKind::ListItem).then_some(*index)
                    })
                    .ok_or(MarkdownParseError::InvalidStructure)?;
                self.blocks[item_index].task_checked = Some(checked);
                Ok(())
            }
            Event::Html(_)
            | Event::InlineHtml(_)
            | Event::InlineMath(_)
            | Event::DisplayMath(_)
            | Event::FootnoteReference(_)
            | Event::SoftBreak
            | Event::HardBreak => Ok(()),
        }
    }

    fn start(&mut self, tag: Tag<'a>, span: SourceSpan) -> Result<(), MarkdownParseError> {
        self.container_depth = self
            .container_depth
            .checked_add(1)
            .ok_or(MarkdownParseError::InvalidStructure)?;
        if self.container_depth > self.limits.max_nesting_depth {
            return Err(MarkdownParseError::ResourceLimit(
                ParseResource::NestingDepth,
            ));
        }

        match &tag {
            Tag::Link {
                link_type,
                dest_url,
                ..
            } => {
                self.push_relation(*link_type, false, dest_url, span)?;
                self.link_depth += 1;
            }
            Tag::Image {
                link_type,
                dest_url,
                ..
            } => {
                self.push_relation(*link_type, true, dest_url, span)?;
                self.link_depth += 1;
            }
            Tag::CodeBlock(_) => self.code_depth += 1,
            Tag::HtmlBlock => self.html_depth += 1,
            Tag::MetadataBlock(_) => self.metadata_depth += 1,
            _ => {}
        }

        if let Some(spec) = block_spec(&tag) {
            let end = tag.to_end();
            let index = self.push_block(spec, span)?;
            self.open_blocks.push((end, index));
            if matches!(tag, Tag::Heading { .. }) {
                self.heading_capture.push((index, String::new()));
            }
        }
        Ok(())
    }

    fn end(&mut self, end: TagEnd) -> Result<(), MarkdownParseError> {
        if self.container_depth == 0 {
            return Err(MarkdownParseError::InvalidStructure);
        }
        self.container_depth -= 1;

        match end {
            TagEnd::Link | TagEnd::Image => {
                self.link_depth = self
                    .link_depth
                    .checked_sub(1)
                    .ok_or(MarkdownParseError::InvalidStructure)?;
            }
            TagEnd::CodeBlock => {
                self.code_depth = self
                    .code_depth
                    .checked_sub(1)
                    .ok_or(MarkdownParseError::InvalidStructure)?;
            }
            TagEnd::HtmlBlock => {
                self.html_depth = self
                    .html_depth
                    .checked_sub(1)
                    .ok_or(MarkdownParseError::InvalidStructure)?;
            }
            TagEnd::MetadataBlock(_) => {
                self.metadata_depth = self
                    .metadata_depth
                    .checked_sub(1)
                    .ok_or(MarkdownParseError::InvalidStructure)?;
            }
            _ => {}
        }

        if is_block_end(end) {
            let (expected, index) = self
                .open_blocks
                .pop()
                .ok_or(MarkdownParseError::InvalidStructure)?;
            if expected != end {
                return Err(MarkdownParseError::InvalidStructure);
            }
            if matches!(end, TagEnd::Heading(_)) {
                let (heading_index, text) = self
                    .heading_capture
                    .pop()
                    .ok_or(MarkdownParseError::InvalidStructure)?;
                if heading_index != index {
                    return Err(MarkdownParseError::InvalidStructure);
                }
                self.blocks[index].native_locator = Some(MarkdownLocator::Heading {
                    text: text.trim().to_owned(),
                });
            }
        }
        Ok(())
    }

    fn push_block(
        &mut self,
        spec: BlockSpec,
        source_span: SourceSpan,
    ) -> Result<usize, MarkdownParseError> {
        if self.blocks.len() >= self.limits.max_blocks {
            return Err(MarkdownParseError::ResourceLimit(ParseResource::Blocks));
        }
        let local_id = u64::try_from(self.blocks.len() + 1)
            .map_err(|_| MarkdownParseError::InvalidStructure)?;
        let parent_local_id = self
            .open_blocks
            .last()
            .map(|(_, index)| self.blocks[*index].local_id);
        let index = self.blocks.len();
        self.blocks.push(MarkdownBlock {
            local_id,
            parent_local_id,
            ordinal: index,
            kind: spec.kind,
            source_span,
            heading_level: spec.heading_level,
            list_start: spec.list_start,
            task_checked: None,
            info_string: spec.info_string,
            native_locator: None,
        });
        Ok(index)
    }

    fn push_standalone_block(
        &mut self,
        kind: MarkdownBlockKind,
        span: SourceSpan,
    ) -> Result<(), MarkdownParseError> {
        self.push_block(
            BlockSpec {
                kind,
                heading_level: None,
                list_start: None,
                info_string: None,
            },
            span,
        )?;
        Ok(())
    }

    fn push_relation(
        &mut self,
        link_type: LinkType,
        image: bool,
        destination: &str,
        span: SourceSpan,
    ) -> Result<(), MarkdownParseError> {
        let (kind, target, alias, heading, block_id) = match link_type {
            LinkType::WikiLink { .. } => {
                let raw = span
                    .slice(self.source)
                    .ok_or(MarkdownParseError::InvalidStructure)?;
                let Some(parsed) = parse_wikilink(raw, image) else {
                    return Ok(());
                };
                (
                    if image {
                        MarkdownRelationKind::Embed
                    } else {
                        MarkdownRelationKind::Wikilink
                    },
                    parsed.target,
                    parsed.alias,
                    parsed.heading,
                    parsed.block_id,
                )
            }
            LinkType::Autolink | LinkType::Email => (
                MarkdownRelationKind::Autolink,
                destination.to_owned(),
                None,
                None,
                None,
            ),
            _ => (
                if image {
                    MarkdownRelationKind::Image
                } else {
                    MarkdownRelationKind::Link
                },
                destination.to_owned(),
                None,
                None,
                None,
            ),
        };
        self.reserve_link()?;
        self.relations.push(MarkdownRelation {
            kind,
            target,
            alias,
            heading,
            block_id,
            source_span: span,
        });
        Ok(())
    }

    fn reserve_link(&self) -> Result<(), MarkdownParseError> {
        if self.relations.len() >= self.limits.max_links {
            Err(MarkdownParseError::ResourceLimit(ParseResource::Links))
        } else {
            Ok(())
        }
    }

    fn text_extensions_enabled(&self) -> bool {
        self.link_depth == 0
            && self.code_depth == 0
            && self.html_depth == 0
            && self.metadata_depth == 0
    }

    fn scan_literal_autolinks(
        &mut self,
        text: &str,
        absolute_start: usize,
    ) -> Result<(), MarkdownParseError> {
        let mut index = 0;
        while index < text.len() {
            if !text.is_char_boundary(index) {
                return Err(MarkdownParseError::InvalidStructure);
            }
            let preceding = text[..index].chars().next_back();
            let allowed = preceding.is_none_or(gfm_autolinks::check_prev);
            if allowed && let Some((target, consumed)) = gfm_autolinks::match_start(&text[index..])
            {
                self.reserve_link()?;
                let end = index
                    .checked_add(consumed)
                    .ok_or(MarkdownParseError::InvalidStructure)?;
                let source_span = SourceSpan::new(
                    self.source,
                    (absolute_start + index)..(absolute_start + end),
                )?;
                self.relations.push(MarkdownRelation {
                    kind: MarkdownRelationKind::Autolink,
                    target,
                    alias: None,
                    heading: None,
                    block_id: None,
                    source_span,
                });
                index = end;
                continue;
            }
            index += text[index..].chars().next().map_or(1, char::len_utf8);
        }
        Ok(())
    }

    fn scan_inline_tags(
        &mut self,
        text: &str,
        absolute_start: usize,
    ) -> Result<(), MarkdownParseError> {
        for (hash_index, _) in text.match_indices('#') {
            let preceding = text[..hash_index].chars().next_back();
            if preceding.is_some_and(|value| !gfm_autolinks::check_prev(value)) {
                continue;
            }
            let value_start = hash_index + 1;
            let mut value_end = value_start;
            for (relative, character) in text[value_start..].char_indices() {
                if !valid_tag_character(character) {
                    break;
                }
                value_end = value_start + relative + character.len_utf8();
            }
            let value = &text[value_start..value_end];
            if value.is_empty() || !value.chars().any(|character| !character.is_numeric()) {
                continue;
            }
            self.tags.push(MarkdownTag {
                value: value.to_owned(),
                source_span: SourceSpan::new(
                    self.source,
                    (absolute_start + hash_index)..(absolute_start + value_end),
                )?,
            });
        }
        Ok(())
    }

    fn finish(mut self) -> Result<ParsedMarkdownV1, MarkdownParseError> {
        if self.container_depth != 0
            || !self.open_blocks.is_empty()
            || !self.heading_capture.is_empty()
            || self.link_depth != 0
            || self.code_depth != 0
            || self.html_depth != 0
            || self.metadata_depth != 0
        {
            return Err(MarkdownParseError::InvalidStructure);
        }
        attach_block_id_locators(self.source, &mut self.blocks)?;
        let spans = self
            .properties
            .iter()
            .map(|property| property.source_span)
            .chain(self.blocks.iter().map(|block| block.source_span))
            .chain(self.relations.iter().map(|relation| relation.source_span))
            .chain(self.tags.iter().map(|tag| tag.source_span));
        for span in spans {
            span.slice(self.source)
                .ok_or(MarkdownParseError::InvalidStructure)?;
        }
        Ok(ParsedMarkdownV1 {
            contract_version: CONTRACT_VERSION.to_owned(),
            properties: self.properties,
            blocks: self.blocks,
            relations: self.relations,
            tags: self.tags,
        })
    }
}

struct BlockSpec {
    kind: MarkdownBlockKind,
    heading_level: Option<u8>,
    list_start: Option<u64>,
    info_string: Option<String>,
}

fn block_spec(tag: &Tag<'_>) -> Option<BlockSpec> {
    let (kind, heading_level, list_start, info_string) = match tag {
        Tag::Paragraph => (MarkdownBlockKind::Paragraph, None, None, None),
        Tag::Heading { level, .. } => (
            MarkdownBlockKind::Heading,
            Some(heading_level(*level)),
            None,
            None,
        ),
        Tag::BlockQuote(_) => (MarkdownBlockKind::BlockQuote, None, None, None),
        Tag::CodeBlock(kind) => (
            MarkdownBlockKind::CodeBlock,
            None,
            None,
            match kind {
                CodeBlockKind::Indented => None,
                CodeBlockKind::Fenced(value) => Some(value.to_string()),
            },
        ),
        Tag::HtmlBlock => (MarkdownBlockKind::HtmlBlock, None, None, None),
        Tag::List(start) => (MarkdownBlockKind::List, None, *start, None),
        Tag::Item => (MarkdownBlockKind::ListItem, None, None, None),
        Tag::Table(_) => (MarkdownBlockKind::Table, None, None, None),
        Tag::TableHead => (MarkdownBlockKind::TableHead, None, None, None),
        Tag::TableRow => (MarkdownBlockKind::TableRow, None, None, None),
        Tag::TableCell => (MarkdownBlockKind::TableCell, None, None, None),
        Tag::MetadataBlock(MetadataBlockKind::YamlStyle) => {
            (MarkdownBlockKind::MetadataBlock, None, None, None)
        }
        Tag::MetadataBlock(MetadataBlockKind::PlusesStyle)
        | Tag::FootnoteDefinition(_)
        | Tag::DefinitionList
        | Tag::DefinitionListTitle
        | Tag::DefinitionListDefinition
        | Tag::Emphasis
        | Tag::Strong
        | Tag::Strikethrough
        | Tag::Superscript
        | Tag::Subscript
        | Tag::Link { .. }
        | Tag::Image { .. } => return None,
    };
    Some(BlockSpec {
        kind,
        heading_level,
        list_start,
        info_string,
    })
}

const fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

const fn is_block_end(end: TagEnd) -> bool {
    matches!(
        end,
        TagEnd::Paragraph
            | TagEnd::Heading(_)
            | TagEnd::BlockQuote(_)
            | TagEnd::CodeBlock
            | TagEnd::HtmlBlock
            | TagEnd::List(_)
            | TagEnd::Item
            | TagEnd::Table
            | TagEnd::TableHead
            | TagEnd::TableRow
            | TagEnd::TableCell
            | TagEnd::MetadataBlock(MetadataBlockKind::YamlStyle)
    )
}

struct Frontmatter {
    whole: Range<usize>,
    body: Range<usize>,
}

fn find_frontmatter(source: &str) -> Result<Option<Frontmatter>, MarkdownParseError> {
    let Some((first_end, first_next)) = next_line(source, 0) else {
        return Ok(None);
    };
    if &source[..first_end] != "---" {
        return Ok(None);
    }
    let mut cursor = first_next;
    while let Some((line_end, next)) = next_line(source, cursor) {
        let line = &source[cursor..line_end];
        if matches!(line, "---" | "...") {
            return Ok(Some(Frontmatter {
                whole: 0..next,
                body: first_next..cursor,
            }));
        }
        cursor = next;
    }
    Ok(None)
}

fn next_line(source: &str, start: usize) -> Option<(usize, usize)> {
    if start >= source.len() {
        return None;
    }
    let relative = source[start..].find('\n');
    let next = relative.map_or(source.len(), |value| start + value + 1);
    let mut end = relative.map_or(source.len(), |value| start + value);
    if end > start && source.as_bytes()[end - 1] == b'\r' {
        end -= 1;
    }
    Some((end, next))
}

fn parse_properties(
    source: &str,
    frontmatter: &Frontmatter,
) -> (Vec<MarkdownProperty>, Vec<MarkdownTag>) {
    let body = &source[frontmatter.body.clone()];
    let Ok(documents) = Yaml::load_from_str(body) else {
        return (Vec::new(), Vec::new());
    };
    let Some(mapping) = documents.first().and_then(Yaml::as_mapping) else {
        return (Vec::new(), Vec::new());
    };
    let property_sources = top_level_property_sources(body);
    let mut properties = Vec::new();
    let mut tags = Vec::new();
    for (key, value) in mapping {
        let Some(name) = key.as_str() else {
            continue;
        };
        if name == "<<"
            || property_sources
                .get(name)
                .is_some_and(|source| contains_yaml_reference_syntax(source))
        {
            continue;
        }
        let Some(values) = scalar_values(value) else {
            continue;
        };
        let source_span = SourceSpan {
            start_byte: frontmatter.whole.start,
            end_byte: frontmatter.whole.end,
        };
        if name.eq_ignore_ascii_case("tags") {
            for value in &values {
                let normalized = value.strip_prefix('#').unwrap_or(value);
                if valid_tag(normalized) {
                    tags.push(MarkdownTag {
                        value: normalized.to_owned(),
                        source_span,
                    });
                }
            }
        }
        properties.push(MarkdownProperty {
            name: name.to_owned(),
            values,
            source_span,
        });
    }
    (properties, tags)
}

fn scalar_values(value: &Yaml<'_>) -> Option<Vec<String>> {
    match value {
        Yaml::Value(value) => scalar_value(value).map(|value| vec![value]),
        Yaml::Sequence(values) => values
            .iter()
            .map(|value| match value {
                Yaml::Value(value) => scalar_value(value),
                _ => None,
            })
            .collect(),
        Yaml::Representation(value, _, None) => Some(vec![value.to_string()]),
        Yaml::Representation(_, _, Some(_))
        | Yaml::Mapping(_)
        | Yaml::Tagged(_, _)
        | Yaml::Alias(_)
        | Yaml::BadValue => None,
    }
}

fn scalar_value(value: &Scalar<'_>) -> Option<String> {
    Some(match value {
        Scalar::Null => "null".to_owned(),
        Scalar::Boolean(value) => value.to_string(),
        Scalar::Integer(value) => value.to_string(),
        Scalar::FloatingPoint(value) if value.is_finite() => value.to_string(),
        Scalar::FloatingPoint(_) => return None,
        Scalar::String(value) => value.to_string(),
    })
}

fn top_level_property_sources(body: &str) -> std::collections::HashMap<String, &str> {
    let mut entries = Vec::<(String, usize)>::new();
    let mut cursor = 0;
    while let Some((line_end, next)) = next_line(body, cursor) {
        let line = &body[cursor..line_end];
        if !line.starts_with(char::is_whitespace)
            && !line.starts_with('#')
            && let Some((name, _)) = line.split_once(':')
        {
            let name = name.trim();
            if !name.is_empty() && !name.starts_with(['\'', '"']) {
                entries.push((name.to_owned(), cursor));
            }
        }
        cursor = next;
    }
    let mut result = std::collections::HashMap::new();
    for (index, (name, start)) in entries.iter().enumerate() {
        let end = entries
            .get(index + 1)
            .map_or(body.len(), |(_, next_start)| *next_start);
        result.insert(name.clone(), &body[*start..end]);
    }
    result
}

fn contains_yaml_reference_syntax(source: &str) -> bool {
    if source
        .lines()
        .any(|line| line.trim_start().starts_with("<<:"))
    {
        return true;
    }
    let bytes = source.as_bytes();
    bytes.iter().enumerate().any(|(index, byte)| {
        matches!(byte, b'&' | b'*')
            && (index == 0
                || bytes[index - 1].is_ascii_whitespace()
                || matches!(bytes[index - 1], b'[' | b','))
            && bytes
                .get(index + 1)
                .is_some_and(|next| next.is_ascii_alphanumeric() || matches!(next, b'_' | b'-'))
    })
}

struct ParsedWikiLink {
    target: String,
    alias: Option<String>,
    heading: Option<String>,
    block_id: Option<String>,
}

fn parse_wikilink(raw: &str, image: bool) -> Option<ParsedWikiLink> {
    let body = if image {
        raw.strip_prefix("![[")?.strip_suffix("]]")?
    } else {
        raw.strip_prefix("[[")?.strip_suffix("]]")?
    };
    let (target_and_fragment, alias) = body
        .split_once('|')
        .map_or((body, None), |(target, alias)| (target, Some(alias.trim())));
    if alias == Some("") {
        return None;
    }
    let (target, fragment) = target_and_fragment
        .split_once('#')
        .map_or((target_and_fragment, None), |(target, fragment)| {
            (target, Some(fragment))
        });
    let target = target.trim();
    if target
        .chars()
        .any(|character| matches!(character, '|' | '^' | ':' | '[' | ']') || character == '%')
    {
        return None;
    }
    let (heading, block_id) = match fragment {
        Some(fragment) if fragment.starts_with('^') => {
            let id = fragment.strip_prefix('^')?;
            if !valid_block_id(id) {
                return None;
            }
            (None, Some(id.to_owned()))
        }
        Some("") => return None,
        Some(fragment) => (Some(fragment.to_owned()), None),
        None => (None, None),
    };
    if target.is_empty() && heading.is_none() && block_id.is_none() {
        return None;
    }
    Some(ParsedWikiLink {
        target: target.to_owned(),
        alias: alias.map(str::to_owned),
        heading,
        block_id,
    })
}

fn attach_block_id_locators(
    source: &str,
    blocks: &mut [MarkdownBlock],
) -> Result<(), MarkdownParseError> {
    let standalone = blocks
        .iter()
        .enumerate()
        .filter_map(|(index, block)| {
            (block.kind == MarkdownBlockKind::Paragraph)
                .then(|| block.source_span.slice(source))
                .flatten()
                .and_then(|raw| {
                    let id = raw.trim().strip_prefix('^')?;
                    valid_block_id(id).then_some((index, id.to_owned()))
                })
        })
        .collect::<Vec<_>>();
    let mut assigned = HashSet::new();
    for (marker_index, id) in standalone {
        let marker_start = blocks[marker_index].source_span.start_byte;
        let target = blocks
            .iter()
            .enumerate()
            .filter(|(index, block)| {
                *index != marker_index
                    && block.parent_local_id.is_none()
                    && block.source_span.end_byte <= marker_start
                    && block.kind != MarkdownBlockKind::MetadataBlock
            })
            .max_by_key(|(_, block)| block.source_span.end_byte)
            .map(|(index, _)| index);
        if let Some(target) = target
            && blocks[target].native_locator.is_none()
        {
            blocks[target].native_locator = Some(MarkdownLocator::BlockId { id: id.clone() });
            assigned.insert(id);
        }
    }

    let mut candidates = blocks
        .iter()
        .enumerate()
        .filter_map(|(index, block)| {
            matches!(
                block.kind,
                MarkdownBlockKind::Paragraph | MarkdownBlockKind::ListItem
            )
            .then_some((
                index,
                block.source_span.end_byte - block.source_span.start_byte,
            ))
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|(_, length)| *length);
    for (index, _) in candidates {
        let raw = blocks[index]
            .source_span
            .slice(source)
            .ok_or(MarkdownParseError::InvalidStructure)?;
        if raw.trim().starts_with('^') {
            continue;
        }
        if let Some(id) = trailing_block_id(raw)
            && assigned.insert(id.to_owned())
        {
            blocks[index].native_locator = Some(MarkdownLocator::BlockId { id: id.to_owned() });
        }
    }
    Ok(())
}

fn trailing_block_id(source: &str) -> Option<&str> {
    let trimmed = source.trim_end();
    let caret = trimmed.rfind('^')?;
    if caret > 0 && !trimmed[..caret].chars().next_back()?.is_whitespace() {
        return None;
    }
    let id = &trimmed[caret + 1..];
    valid_block_id(id).then_some(id)
}

fn valid_block_id(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn valid_tag(value: &str) -> bool {
    !value.is_empty()
        && value.chars().all(valid_tag_character)
        && value.chars().any(|character| !character.is_numeric())
}

fn valid_tag_character(character: char) -> bool {
    character.is_alphanumeric()
        || matches!(character, '_' | '-' | '/')
        || (!character.is_whitespace() && !character.is_ascii_punctuation())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hard_ceilings_cannot_be_relaxed() {
        let cases = [
            (
                ParseLimits::new(
                    MAX_SOURCE_BYTES + 1,
                    MAX_BLOCKS,
                    MAX_NESTING_DEPTH,
                    MAX_METADATA_BYTES,
                    MAX_LINKS,
                ),
                ParseResource::SourceBytes,
                MAX_SOURCE_BYTES,
            ),
            (
                ParseLimits::new(
                    MAX_SOURCE_BYTES,
                    MAX_BLOCKS + 1,
                    MAX_NESTING_DEPTH,
                    MAX_METADATA_BYTES,
                    MAX_LINKS,
                ),
                ParseResource::Blocks,
                MAX_BLOCKS,
            ),
            (
                ParseLimits::new(
                    MAX_SOURCE_BYTES,
                    MAX_BLOCKS,
                    MAX_NESTING_DEPTH + 1,
                    MAX_METADATA_BYTES,
                    MAX_LINKS,
                ),
                ParseResource::NestingDepth,
                MAX_NESTING_DEPTH,
            ),
            (
                ParseLimits::new(
                    MAX_SOURCE_BYTES,
                    MAX_BLOCKS,
                    MAX_NESTING_DEPTH,
                    MAX_METADATA_BYTES + 1,
                    MAX_LINKS,
                ),
                ParseResource::MetadataBytes,
                MAX_METADATA_BYTES,
            ),
            (
                ParseLimits::new(
                    MAX_SOURCE_BYTES,
                    MAX_BLOCKS,
                    MAX_NESTING_DEPTH,
                    MAX_METADATA_BYTES,
                    MAX_LINKS + 1,
                ),
                ParseResource::Links,
                MAX_LINKS,
            ),
        ];

        for (result, resource, hard_maximum) in cases {
            assert_eq!(
                result,
                Err(ParseLimitError {
                    resource,
                    hard_maximum,
                })
            );
        }
    }

    #[test]
    fn wikilink_disambiguation_is_stable() {
        let parsed = parse_wikilink("![[note#^block-id|shown]]", true).unwrap();
        assert_eq!(parsed.target, "note");
        assert_eq!(parsed.alias.as_deref(), Some("shown"));
        assert_eq!(parsed.heading, None);
        assert_eq!(parsed.block_id.as_deref(), Some("block-id"));
    }
}
