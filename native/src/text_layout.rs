use crate::text::{BUNDLED, glyph_outline};
use crate::text_animator::{TextLayout, Units};
use fontdb::{Database, Family, Query, Style, Weight};
use rustybuzz::{Direction, Face, UnicodeBuffer};
use std::collections::{BTreeMap, HashMap};
use std::ops::Range;
use std::sync::{Arc, OnceLock};
use tiny_skia::Path;
use ttf_parser::GlyphId;
use unicode_bidi::ParagraphBidiInfo;
use unicode_script::{Script, UnicodeScript};
use unicode_segmentation::UnicodeSegmentation;

pub(crate) const BUNDLED_FAMILY: &str = "DM Sans (bundled)";

struct SystemFonts {
    db: Database,
    families: Vec<String>,
}

fn system_fonts() -> &'static SystemFonts {
    static FONTS: OnceLock<SystemFonts> = OnceLock::new();
    FONTS.get_or_init(|| {
        let mut db = Database::new();
        db.load_system_fonts();
        let mut families: Vec<_> = db
            .faces()
            .filter_map(|face| face.families.first().map(|(name, _)| name.clone()))
            .collect();
        families.sort_unstable_by_key(|name| name.to_lowercase());
        families.dedup();
        SystemFonts { db, families }
    })
}

pub(crate) fn font_families() -> &'static [String] {
    &system_fonts().families
}

pub(crate) fn available_styles(family: &str) -> (bool, bool) {
    if family == BUNDLED_FAMILY {
        return (false, false);
    }
    let mut bold = false;
    let mut italic = false;
    for face in system_fonts()
        .db
        .faces()
        .filter(|face| face.families.iter().any(|(name, _)| name == family))
    {
        bold |= face.weight >= Weight::SEMIBOLD;
        italic |= face.style != Style::Normal;
    }
    (bold, italic)
}

fn query(name: &str, bold: bool, italic: bool) -> Option<FontId> {
    system_fonts()
        .db
        .query(&Query {
            families: &[Family::Name(name)],
            weight: if bold { Weight::BOLD } else { Weight::NORMAL },
            style: if italic { Style::Italic } else { Style::Normal },
            ..Query::default()
        })
        .map(FontId::System)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum FontId {
    Bundled(usize),
    System(fontdb::ID),
}

type SystemFace = (Arc<[u8]>, u32);
type GlyphOutlines = HashMap<(FontId, u16), Option<Arc<Path>>>;

#[derive(Default)]
pub(crate) struct TextCache {
    fonts: HashMap<fontdb::ID, Option<SystemFace>>,
    outlines: GlyphOutlines,
    layouts: HashMap<String, CachedLayout>,
}

#[derive(PartialEq, Eq)]
struct FontChoice {
    family: Option<String>,
    bold: bool,
    italic: bool,
}

struct CachedLayout {
    content: String,
    font: FontChoice,
    shaped: Arc<ShapedText>,
}

#[derive(Default)]
pub(crate) struct ShapedText {
    pub lines: Vec<Line>,
    pub width: f32,
    pub baseline: f32,
    counts: [usize; 4],
}

#[derive(Default)]
pub(crate) struct Line {
    pub width: f32,
    pub glyphs: Vec<Glyph>,
    pub clusters: Vec<Cluster>,
}

pub(crate) struct Glyph {
    pub path: Option<Arc<Path>>,
    pub x: f32,
    pub y: f32,
}

#[derive(Default)]
pub(crate) struct Cluster {
    pub x: f32,
    pub advance: f32,
    pub glyphs: Range<usize>,
    units: [Option<usize>; 4],
}

impl Cluster {
    pub fn unit(&self, units: Units) -> Option<usize> {
        self.units[unit_index(units)]
    }
}

impl ShapedText {
    pub fn count(&self, units: Units) -> usize {
        self.counts[unit_index(units)]
    }
}

fn unit_index(units: Units) -> usize {
    match units {
        Units::Characters => 0,
        Units::VisibleCharacters => 1,
        Units::Words => 2,
        Units::Lines => 3,
    }
}

impl TextCache {
    pub fn layout(&mut self, id: &str, content: &str, style: &TextLayout) -> Arc<ShapedText> {
        let font = FontChoice {
            family: style.font.clone(),
            bold: style.bold,
            italic: style.italic,
        };
        if let Some(cached) = self.layouts.get(id)
            && cached.content == content
            && cached.font == font
        {
            return cached.shaped.clone();
        }
        if self.layouts.len() >= 128 && !self.layouts.contains_key(id) {
            self.layouts.clear();
        }
        if self.outlines.len() >= 4096 {
            self.outlines.clear();
        }
        let shaped = Arc::new(self.shape(content, &font));
        self.layouts.insert(
            id.to_owned(),
            CachedLayout {
                content: content.to_owned(),
                font,
                shaped: shaped.clone(),
            },
        );
        shaped
    }

    fn face_data(&mut self, font: FontId) -> Option<(FontBytes, u32)> {
        match font {
            FontId::Bundled(index) => Some((FontBytes::Bundled(BUNDLED[index]), 0)),
            FontId::System(id) => {
                if self.fonts.len() >= 32 && !self.fonts.contains_key(&id) {
                    self.fonts.clear();
                }
                let (bytes, index) = self
                    .fonts
                    .entry(id)
                    .or_insert_with(|| {
                        system_fonts()
                            .db
                            .with_face_data(id, |bytes, index| (Arc::from(bytes), index))
                    })
                    .as_ref()?;
                Some((FontBytes::System(bytes.clone()), *index))
            }
        }
    }

    fn shape(&mut self, content: &str, choice: &FontChoice) -> ShapedText {
        let primary = choice
            .family
            .as_deref()
            .and_then(|name| query(name, choice.bold, choice.italic))
            .unwrap_or(FontId::Bundled(0));
        let mut fonts = LayoutFonts::new(self, primary, choice);
        let baseline = fonts.faces.first().map_or(0.0, |font| {
            font.with_face(|face| {
                let (ascent, descent) = (f32::from(face.ascender()), -f32::from(face.descender()));
                (ascent - descent) / (2.0 * (ascent + descent).max(1.0))
            })
            .unwrap_or(0.0)
        });
        let mut text = ShapedText {
            baseline,
            ..ShapedText::default()
        };
        for (index, line) in content.split('\n').enumerate() {
            let line = line.trim_end_matches('\r');
            let (mut layout, clusters) = shape_line(line, &mut fonts, self);
            index_units(line, &mut layout, &clusters, index, &mut text.counts);
            text.width = text.width.max(layout.width);
            text.lines.push(layout);
        }
        text.counts[unit_index(Units::Lines)] = text.lines.len();
        text
    }
}

#[derive(Clone)]
enum FontBytes {
    Bundled(&'static [u8]),
    System(Arc<[u8]>),
}

impl FontBytes {
    fn as_slice(&self) -> &[u8] {
        match self {
            Self::Bundled(data) => data,
            Self::System(data) => data,
        }
    }
}

struct LayoutFont {
    id: FontId,
    data: FontBytes,
    index: u32,
}

impl LayoutFont {
    fn with_face<T>(&self, f: impl FnOnce(&Face<'_>) -> T) -> Option<T> {
        Face::from_slice(self.data.as_slice(), self.index)
            .as_ref()
            .map(f)
    }

    fn covers(&self, grapheme: &str) -> bool {
        self.with_face(|face| {
            grapheme.chars().all(|c| {
                c.is_control()
                    || matches!(c, '\u{200c}' | '\u{200d}' | '\u{fe0e}' | '\u{fe0f}')
                    || face.glyph_index(c).is_some()
            })
        })
        .unwrap_or(false)
    }
}

struct LayoutFonts<'a> {
    faces: Vec<LayoutFont>,
    choice: &'a FontChoice,
    tried_fallbacks: bool,
    choices: HashMap<String, usize>,
}

impl<'a> LayoutFonts<'a> {
    fn new(cache: &mut TextCache, primary: FontId, choice: &'a FontChoice) -> Self {
        let mut fonts = Self {
            faces: Vec::new(),
            choice,
            tried_fallbacks: false,
            choices: HashMap::new(),
        };
        for id in [primary, FontId::Bundled(0), FontId::Bundled(1)] {
            fonts.add(cache, id);
        }
        fonts
    }

    fn add(&mut self, cache: &mut TextCache, id: FontId) {
        if !self.faces.iter().any(|font| font.id == id)
            && let Some((data, index)) = cache.face_data(id)
        {
            self.faces.push(LayoutFont { id, data, index });
        }
    }

    fn select(&mut self, cache: &mut TextCache, grapheme: &str) -> usize {
        if let Some(&index) = self.choices.get(grapheme) {
            return index;
        }
        let mut index = self.faces.iter().position(|font| font.covers(grapheme));
        if index.is_none() && !self.tried_fallbacks {
            self.tried_fallbacks = true;
            for name in [
                "Segoe UI",
                "Arial",
                "Segoe UI Symbol",
                "Segoe UI Emoji",
                "Microsoft YaHei",
                "Yu Gothic",
                "Malgun Gothic",
                "Nirmala UI",
            ] {
                if let Some(id) = query(name, self.choice.bold, self.choice.italic) {
                    self.add(cache, id);
                }
            }
            index = self.faces.iter().position(|font| font.covers(grapheme));
        }
        let index = index.unwrap_or(0);
        self.choices.insert(grapheme.into(), index);
        index
    }
}

struct Run {
    bytes: Range<usize>,
    font: usize,
    script: Option<Script>,
}

fn shape_line(
    line: &str,
    fonts: &mut LayoutFonts<'_>,
    cache: &mut TextCache,
) -> (Line, Vec<usize>) {
    if line.is_empty() {
        return (Line::default(), Vec::new());
    }
    let bidi = ParagraphBidiInfo::new(line, None);
    let (levels, visual_runs) = bidi.visual_runs(0..line.len());
    let mut layout = Line::default();
    let mut clusters = Vec::new();
    // Bidi byte ranges may split a grapheme at a formatting mark. Assign it by its first byte.
    let graphemes: Vec<_> = line.grapheme_indices(true).collect();
    for bytes in visual_runs {
        let rtl = levels.get(bytes.start).is_some_and(|level| level.is_rtl());
        let mut runs: Vec<Run> = Vec::new();
        for &(start, grapheme) in &graphemes {
            if !bytes.contains(&start) {
                continue;
            }
            let font = fonts.select(cache, grapheme);
            let script = grapheme
                .chars()
                .map(|c| c.script())
                .find(|s| !matches!(s, Script::Common | Script::Inherited | Script::Unknown));
            let end = start + grapheme.len();
            if let Some(run) = runs.last_mut()
                && run.font == font
                && (run.script.is_none() || script.is_none() || run.script == script)
            {
                run.bytes.end = end;
                run.script = run.script.or(script);
            } else {
                runs.push(Run {
                    bytes: start..end,
                    font,
                    script,
                });
            }
        }
        if rtl {
            runs.reverse();
        }
        for run in runs {
            if let Some(font) = fonts.faces.get(run.font) {
                font.with_face(|face| {
                    append_run(
                        face,
                        font.id,
                        line,
                        run.bytes,
                        rtl,
                        &mut layout,
                        &mut clusters,
                        &mut cache.outlines,
                    );
                });
            }
        }
    }
    (layout, clusters)
}

#[allow(clippy::too_many_arguments)]
fn append_run(
    face: &Face<'_>,
    font: FontId,
    text: &str,
    bytes: Range<usize>,
    rtl: bool,
    line: &mut Line,
    clusters: &mut Vec<usize>,
    outlines: &mut GlyphOutlines,
) {
    let mut buffer = UnicodeBuffer::new();
    buffer.push_str(&text[bytes.clone()]);
    buffer.set_direction(if rtl {
        Direction::RightToLeft
    } else {
        Direction::LeftToRight
    });
    buffer.guess_segment_properties();
    let shaped = rustybuzz::shape(face, &[], buffer);
    let em = 1.0 / face.units_per_em() as f32;
    let mut previous = None;
    for (info, pos) in shaped.glyph_infos().iter().zip(shaped.glyph_positions()) {
        let byte = bytes.start + info.cluster as usize;
        if previous != Some(byte) {
            clusters.push(byte);
            let at = line.glyphs.len();
            line.clusters.push(Cluster {
                x: line.width,
                glyphs: at..at,
                ..Cluster::default()
            });
            previous = Some(byte);
        }
        let id = info.glyph_id as u16;
        let path = outlines
            .entry((font, id))
            .or_insert_with(|| glyph_outline(face, GlyphId(id)).map(Arc::new));
        line.glyphs.push(Glyph {
            path: path.clone(),
            x: line.width + pos.x_offset as f32 * em,
            y: -pos.y_offset as f32 * em,
        });
        let advance = pos.x_advance as f32 * em;
        line.width += advance;
        if let Some(cluster) = line.clusters.last_mut() {
            cluster.advance += advance;
            cluster.glyphs.end += 1;
        }
    }
}

fn index_units(
    text: &str,
    line: &mut Line,
    bytes: &[usize],
    line_index: usize,
    counts: &mut [usize; 4],
) {
    let mut order: Vec<_> = bytes.iter().copied().enumerate().collect();
    order.sort_unstable_by_key(|&(_, byte)| byte);
    let words: Vec<_> = text
        .unicode_word_indices()
        .map(|(start, word)| start..start + word.len())
        .collect();
    let mut word_ids = BTreeMap::new();
    for (index, &(visual, start)) in order.iter().enumerate() {
        let end = order.get(index + 1).map_or(text.len(), |&(_, end)| end);
        let Some(cluster_text) = text.get(start..end) else {
            continue;
        };
        let visible = cluster_text
            .chars()
            .any(|c| !c.is_whitespace() && !c.is_control());
        let letter = counts[0];
        counts[0] += 1;
        let visible_letter = visible.then(|| {
            counts[1] += 1;
            counts[1] - 1
        });
        let word = visible
            .then(|| {
                let next = words.partition_point(|word| word.end <= start);
                if words.get(next).is_some_and(|word| word.start < end) || next == 0 {
                    words.get(next).map(|_| next)
                } else {
                    Some(next - 1)
                }
            })
            .flatten()
            .map(|word| {
                *word_ids.entry(word).or_insert_with(|| {
                    counts[2] += 1;
                    counts[2] - 1
                })
            });
        line.clusters[visual].units = [Some(letter), visible_letter, word, Some(line_index)];
    }
}
