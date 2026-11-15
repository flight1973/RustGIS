//! A box where an expression is written, with suggestions as it is typed (#1240): the shared
//! list (`completion`) under the cursor, holding what the language takes there as the parser
//! reads it (`query::complete_at`): the dataset's fields, its functions, variables and keywords,
//! each with its help beside. It opens once a character of the word is typed, or with
//! Ctrl+Space. Nothing is listed here by hand: what the language learns is offered too.

use egui::text::{CCursor, CCursorRange};
use egui::widgets::text_edit::{TextEditOutput, TextEditState};
use egui::{Id, Key, Modifiers, Ui};
use rustgis_engine::data::Dataset;
use rustgis_engine::data::query::{self, FuncSpec, Kw, Place, VARS};

use crate::completion::{GAP, List, Offered, Worked};

/// The most fields the list holds (typing narrows them; it scrolls): a table of hundreds of
/// columns mustn't draw hundreds of rows. The functions, variables and keywords after them are
/// the language's, a few dozen, and always all there.
const MAX_FIELDS: usize = 50;

/// What an item stands for.
#[derive(Clone, Debug, PartialEq)]
enum Item {
    /// A field, by its name: what's written is what was offered, whatever the data does
    /// meanwhile.
    Field(String),
    Function(FuncSpec),
    /// A variable (its entry in `VARS`) by the name it was matched by.
    Variable(usize, &'static str),
    Keyword(Kw),
}

/// What to offer at a cursor: the word it replaces (chars), the quote a field name was begun
/// with, and the items, as the list shows them and as what they stand for.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Offer {
    start: usize,
    end: usize,
    quote: Option<char>,
    items: Vec<String>,
    of: Vec<Item>,
}

impl Offered for Offer {
    fn items(&self) -> &[String] {
        &self.items
    }

    fn end(&self) -> usize {
        self.end
    }
}

/// A field as an expression names it: as it is when it reads as a plain name, else in double
/// quotes (a space, a leading digit, a keyword of the language).
pub(crate) fn field_ref(name: &str) -> String {
    let plain = name.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_') && name.chars().all(|c| c.is_alphanumeric() || c == '_');
    let keyword = Kw::ALL.iter().any(|k| k.word().eq_ignore_ascii_case(name));
    if plain && !keyword { name.to_string() } else { format!("\"{}\"", name.replace('"', "\"\"")) }
}

/// Whether `name` starts with `prefix`, letter case aside.
fn starts_with(name: &str, prefix: &str) -> bool {
    name.to_lowercase().starts_with(&prefix.to_lowercase())
}

/// What to offer at char `at` of `text`, an expression over `ds`: what `complete_at` says goes
/// there that starts with the word typed (the word typed whole isn't offered again). Only once
/// a character of the word is typed (a quote begun counts), unless `asked` (Ctrl+Space).
fn offer_at(text: &str, at: usize, ds: &Dataset, asked: bool) -> Option<Offer> {
    let c = query::complete_at(text, at, ds)?;
    if c.place == Place::Text || (c.word.is_empty() && !asked) {
        return None;
    }
    let p = c.prefix.as_str();
    let fits = |name: &str| starts_with(name, p) && !name.eq_ignore_ascii_case(p);
    let mut of = Vec::new();
    if c.operand {
        // A field another one shadows (`name` after `Name`: the expression reads the first, letter
        // case aside) isn't offered: written, it would read the other.
        let mut fields: Vec<&str> = ds
            .fields
            .iter()
            .enumerate()
            .filter(|(i, f)| ds.field_index(&f.name) == Some(*i))
            .map(|(_, f)| f.name.as_str())
            .filter(|n| fits(n))
            .collect();
        fields.sort_by_key(|n| n.to_lowercase());
        fields.truncate(MAX_FIELDS);
        of.extend(fields.into_iter().map(|n| Item::Field(n.to_string())));
    }
    if c.place == Place::Code {
        if c.operand {
            of.extend(query::functions().filter(|f| fits(f.name)).map(Item::Function));
            // A variable once, by the first of its names that fits.
            for (i, v) in VARS.iter().enumerate() {
                if let Some(name) = v.names.iter().find(|n| fits(n)) {
                    of.push(Item::Variable(i, name));
                }
            }
        }
        of.extend(c.keywords.iter().filter(|k| fits(k.word())).map(|k| Item::Keyword(*k)));
    }
    if of.is_empty() {
        return None;
    }
    let items = of.iter().map(shown).collect();
    let quote = (c.place == Place::Field).then(|| text.chars().nth(c.word.start)).flatten();
    Some(Offer { start: c.word.start, end: c.word.end, quote, items, of })
}

/// An item as the list shows it, each apart from the others, as it is picked by what it shows:
/// a field as the expression names it (`"NOT"`, `"$area"` and `"Pop Est"` in quotes, so a field
/// never reads as a keyword or variable), a function with its brackets.
fn shown(item: &Item) -> String {
    match item {
        Item::Field(name) => field_ref(name),
        Item::Function(f) => format!("{}()", f.name),
        Item::Variable(_, name) => name.to_string(),
        Item::Keyword(k) => k.word().to_string(),
    }
}

/// The box's state between frames.
#[derive(Clone, Default)]
struct State {
    list: List<Offer>,
    /// What was worked out to offer, for which cursor, text (its hash) and fields (their names'
    /// hash: another layer's offer others) and whether it was asked for.
    worked: Option<Worked<(u64, bool, bool), Option<Offer>>>,
    /// Where Ctrl+Space asked for the list (the text's hash and the cursor): it shows there with
    /// no word typed, until the text or the cursor moves on.
    asked: Option<(u64, usize)>,
    /// Where an item was just accepted (the text's hash and the cursor): nothing is offered there
    /// (`pop` taken mustn't offer `"Pop Est"`, for Enter to take next), until the text or the
    /// cursor moves on, or Ctrl+Space asks.
    accepted: Option<(u64, usize)>,
}

fn load(ui: &Ui, id: Id) -> State {
    ui.data(|d| d.get_temp::<State>(id.with("expr_complete"))).unwrap_or_default()
}

fn store(ui: &Ui, id: Id, state: State) {
    ui.data_mut(|d| d.insert_temp(id.with("expr_complete"), state));
}

/// A single-line box for an expression over `ds` (none: no suggestions), its id `id`, styled by
/// `style` (hint, font, width…), with the suggestion list. Returns the box's response.
pub(crate) fn expr_edit(
    ui: &mut Ui,
    id: Id,
    text: &mut String,
    ds: Option<&Dataset>,
    style: impl for<'t> FnOnce(egui::TextEdit<'t>) -> egui::TextEdit<'t>,
) -> egui::Response {
    let mut state = load(ui, id);
    // The list's keys, taken before the box sees them: Ctrl+Space asks for the list.
    if ui.memory(|m| m.has_focus(id)) && ui.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::Space)) {
        let at = TextEditState::load(ui.ctx(), id).and_then(|s| s.cursor.char_range()).map(|r| r.primary.index.0);
        state.asked = at.map(|at| (egui::util::hash(text.as_str()), at));
        state.list.reopen();
    }
    if state.worked.as_ref().is_some_and(|w| w.text != egui::util::hash(text.as_str())) {
        state.list.forget();
    }
    let picked = state.list.keys(ui, id);
    if let Some((offer, item)) = &picked {
        state.accepted = accept(ui, id, text, offer, item);
        state.worked = None;
    }
    let open = state.list.is_open();
    let filter = egui::EventFilter { tab: open, horizontal_arrows: true, vertical_arrows: true, escape: open };
    let out: TextEditOutput = style(egui::TextEdit::singleline(text).id(id).event_filter(filter)).show(ui);
    let visible = out.text_clip_rect;
    let hash = egui::util::hash(text.as_str());
    let fields = ds.map_or(0, |d| egui::util::hash(d.fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>()));
    let worked = &mut state.worked;
    let asked = &mut state.asked;
    let accepted = &mut state.accepted;
    let work = |at: Option<usize>| {
        // Asked here, and still here.
        let here = at.is_some_and(|at| *asked == Some((hash, at)));
        if !here {
            *asked = None;
        }
        // Just accepted here, and not asked since.
        if at.map(|at| (hash, at)) != *accepted {
            *accepted = None;
        }
        let quiet = accepted.is_some() && !here;
        let with = (fields, here, quiet);
        Worked::get(worked, at, hash, &with).unwrap_or_else(|| {
            let offer = at.zip(ds).filter(|_| !quiet).and_then(|(at, ds)| offer_at(text.as_str(), at, ds, here));
            *worked = Some(Worked { at, text: hash, with, value: offer.clone() });
            offer
        })
    };
    let icon = |offer: &Offer, item: &str| matches!(find(offer, item), Some(Item::Field(_))).then_some("field");
    let draw = |ui: &mut Ui, offer: &Offer, item: &str, width: f32| {
        if let Some(of) = find(offer, item) {
            ui.add_space(GAP);
            draw_help(ui, of, ds, width);
        }
    };
    let clicked = state.list.show(ui, id.with("expr_list"), &out, visible, work, icon, draw);
    store(ui, id, state);
    if let Some((offer, item)) = clicked {
        let accepted = accept(ui, id, text, &offer, &item);
        let mut state = load(ui, id);
        state.accepted = accepted;
        state.worked = None;
        store(ui, id, state);
    }
    out.response.response
}

/// What `item` (as shown) of `offer` stands for.
fn find<'a>(offer: &'a Offer, item: &str) -> Option<&'a Item> {
    offer.items.iter().position(|i| i == item).and_then(|i| offer.of.get(i))
}

/// The help beside the list: a function's signature, coloured as the Script colours one, what
/// it gives and an example; a field's type and alias; what a variable reads or a keyword does.
fn draw_help(ui: &mut Ui, item: &Item, ds: Option<&Dataset>, width: f32) {
    let palette = crate::scriptcolor::Palette::of(&crate::theme::Tokens::get(ui.ctx()));
    let mono = egui::TextStyle::Monospace.resolve(ui.style());
    let code = |ui: &mut Ui, text: &str| {
        let mut job = crate::scriptcolor::expr_job(text, palette, &mono);
        job.wrap.max_width = width;
        ui.label(job);
    };
    ui.vertical(|ui| {
        ui.set_max_width(width);
        match item {
            Item::Function(f) => {
                code(ui, f.signature);
                ui.label(tl!(f.description));
                ui.label(egui::RichText::new(f.example).monospace().small().weak());
            }
            Item::Field(name) => {
                code(ui, &field_ref(name));
                if let Some(f) = ds.and_then(|d| d.fields.iter().find(|f| f.name == *name)) {
                    ui.label(tl!(f.ty.label()));
                    if let Some(alias) = f.alias.as_deref().filter(|a| !a.is_empty()) {
                        ui.label(egui::RichText::new(alias).small());
                    }
                }
            }
            Item::Variable(i, name) => {
                code(ui, name);
                if let Some(v) = VARS.get(*i) {
                    ui.label(tl!(v.description));
                }
            }
            Item::Keyword(k) => {
                code(ui, k.word());
                ui.label(tl!(k.description()));
            }
        }
    });
}

/// Puts `item` in place of the offer's word in `text`, the rest of the word after the cursor
/// too: a field as the expression names it (in the quote it was begun with; a `!` name holding a
/// `!` in double quotes, as `!` has no escape), a function with its bracket open (closed too when
/// it takes no arguments; a `(` already there is used, not doubled), a variable or keyword as it
/// is; as one undo step of the box, the cursor after it, and the box keeps the focus. Returns
/// where it was put: the new text's hash and the cursor.
fn accept(ui: &Ui, id: Id, text: &mut String, offer: &Offer, item: &str) -> Option<(u64, usize)> {
    let (new, at) = written(text, offer, item)?;
    let mut state = TextEditState::load(ui.ctx(), id).unwrap_or_default();
    let mut undoer = state.undoer();
    let was = state.cursor.char_range().unwrap_or(CCursorRange::one(CCursor::new(offer.end)));
    undoer.add_undo(&(was, text.clone()));
    state.set_undoer(undoer);
    *text = new;
    state.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(at))));
    state.store(ui.ctx(), id);
    ui.memory_mut(|m| m.request_focus(id));
    Some((egui::util::hash(text.as_str()), at))
}

/// How many of `rest`, the text after the cursor, are still the word being replaced: the rest of
/// a name or variable (`po▮p`), or of a quoted name with its closing quote (`"Po▮p Est"`; a
/// doubled `"` is one inside the name). None of an unclosed quoted name: what follows is left.
fn word_rest(rest: &[char], quote: Option<char>) -> usize {
    let Some(q) = quote else { return rest.iter().take_while(|c| c.is_alphanumeric() || **c == '_').count() };
    let mut i = 0;
    while let Some(&c) = rest.get(i) {
        if c == q && q == '"' && rest.get(i + 1) == Some(&'"') {
            i += 2;
        } else if c == q {
            return i + 1;
        } else {
            i += 1;
        }
    }
    0
}

/// `text` with `item` of `offer` accepted (`accept`), and where the cursor goes (chars).
fn written(text: &str, offer: &Offer, item: &str) -> Option<(String, usize)> {
    let of = find(offer, item)?;
    let chars: Vec<char> = text.chars().collect();
    let (before, rest) = (chars.get(..offer.start)?, chars.get(offer.end..)?);
    let mut after = rest.get(word_rest(rest, offer.quote)..).unwrap_or_default();
    let put = match of {
        Item::Field(name) => match offer.quote.filter(|q| *q != '!' || !name.contains('!')) {
            Some(q) => {
                let name = if q == '"' { name.replace('"', "\"\"") } else { name.to_string() };
                format!("{q}{name}{q}")
            }
            None => field_ref(name),
        },
        Item::Function(f) if after.first() == Some(&'(') => {
            after = after.get(1..).unwrap_or_default();
            format!("{}(", f.name)
        }
        Item::Function(f) if f.max == 0 => format!("{}()", f.name),
        Item::Function(f) => format!("{}(", f.name),
        Item::Variable(_, name) => name.to_string(),
        Item::Keyword(k) => k.word().to_string(),
    };
    let mut new: String = before.iter().collect();
    new.push_str(&put);
    new.extend(after);
    Some((new, offer.start + put.chars().count()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustgis_engine::data::{Field, FieldType, GeometryType};

    fn ds() -> Dataset {
        Dataset::new(
            "t",
            GeometryType::Polygon,
            vec![Field::new("name", FieldType::Text), Field::new("pop", FieldType::Integer), Field::new("Pop Est", FieldType::Integer)],
        )
    }

    /// The items offered at the end of `text`.
    fn items(text: &str, asked: bool) -> Vec<String> {
        offer_at(text, text.chars().count(), &ds(), asked).map(|o| o.items).unwrap_or_default()
    }

    #[test]
    fn a_field_is_quoted_only_when_an_expression_needs_it() {
        assert_eq!(field_ref("pop_max"), "pop_max");
        assert_eq!(field_ref("Name 2"), "\"Name 2\"");
        assert_eq!(field_ref("2020"), "\"2020\"");
        assert_eq!(field_ref("in"), "\"in\"");
        assert_eq!(field_ref("True"), "\"True\"");
        // Every keyword of the language, not only the ones a clause starts with.
        assert_eq!(field_ref("case"), "\"case\"");
        assert_eq!(field_ref("año"), "año");
        assert_eq!(field_ref("a\"b"), "\"a\"\"b\"");
    }

    /// A word begun: the fields, functions and variables it starts, letter case aside, fields
    /// first; the keywords where a keyword goes.
    #[test]
    fn a_word_begun_offers_what_it_starts() {
        assert_eq!(items("po", false), ["pop", "\"Pop Est\""]);
        assert_eq!(items("ro", false), ["round()"]);
        assert_eq!(items("$ar", false), ["$area"]);
        assert_eq!(items("pop > 1 a", false), ["AND"]);
        assert!(items("pop > 1 AND ", false).is_empty(), "no word typed");
        assert!(items("pop = 'po", false).is_empty(), "inside text");
        assert!(items("name", false).is_empty(), "the field typed whole");
    }

    /// Ctrl+Space with no word typed offers everything that goes there.
    #[test]
    fn asked_with_no_word_offers_all_that_goes_there() {
        let all = items("", true);
        assert_eq!(all.first().map(String::as_str), Some("name"), "fields first");
        assert!(all.contains(&"round()".to_string()) && all.contains(&"$area".to_string()) && all.contains(&"CASE".to_string()));
        assert!(!all.contains(&"AND".to_string()), "no AND where a value goes");
        assert!(items("", false).is_empty(), "not unasked");
    }

    /// In a quoted field name, only fields.
    #[test]
    fn a_quoted_name_offers_only_fields() {
        let o = offer_at("\"Po", 3, &ds(), false).unwrap();
        assert_eq!((o.items, o.quote, o.start), (vec!["pop".to_string(), "\"Pop Est\"".to_string()], Some('"'), 0));
        assert!(items("\"ro", false).is_empty(), "no function in a field's quotes");
    }

    /// A field that reads like a keyword or a variable is shown quoted, so each item is picked
    /// as what it is: Enter on `NOT` writes the keyword, on `"NOT"` the field.
    #[test]
    fn a_field_never_reads_as_a_keyword_or_variable() {
        let ds = Dataset::new("t", GeometryType::Point, vec![Field::new("NOT", FieldType::Text), Field::new("$area", FieldType::Double)]);
        let o = offer_at("no", 2, &ds, false).unwrap();
        assert_eq!(o.items, ["\"NOT\"", "NOT"]);
        assert_eq!(find(&o, "NOT"), Some(&Item::Keyword(Kw::Not)));
        assert_eq!(find(&o, "\"NOT\""), Some(&Item::Field("NOT".into())));
        let o = offer_at("$ar", 3, &ds, false).unwrap();
        assert_eq!(o.items, ["\"$area\"", "$area"]);
    }

    /// A field another one shadows, letter case aside, isn't offered: the expression would read
    /// the first.
    #[test]
    fn a_shadowed_field_is_not_offered() {
        let ds = Dataset::new("t", GeometryType::Point, vec![Field::new("Name", FieldType::Text), Field::new("name", FieldType::Text)]);
        assert_eq!(offer_at("na", 2, &ds, false).map(|o| o.items), Some(vec!["Name".to_string()]));
    }

    /// Accepting closes the list where it was put, though more is on offer there (`pop` taken,
    /// `"Pop Est"` still starts with it): Enter is the dialog's again. Ctrl+Space shows it.
    #[test]
    fn accepting_leaves_the_list_closed_until_asked() {
        use egui::accesskit::Role;
        use egui_kittest::kittest::Queryable;
        let data = ds();
        let id = Id::unique("expr");
        let mut h = egui_kittest::Harness::new_ui_state(
            move |ui, text: &mut String| {
                let label = ui.label("Expression");
                let _ = expr_edit(ui, id, text, Some(&data), |e| e).labelled_by(label.id);
            },
            String::new(),
        );
        h.run();
        h.ctx.memory_mut(|m| m.request_focus(id));
        h.run();
        h.event(egui::Event::Text("po".into()));
        h.run();
        assert!(h.query_by_role_and_label(Role::Button, "pop").is_some(), "offered");
        h.key_press(Key::Enter);
        h.run();
        assert_eq!(h.state(), "pop");
        assert!(h.query_by_role_and_label(Role::Button, "\"Pop Est\"").is_none(), "closed where it was put");
        h.key_press_modifiers(Modifiers::CTRL, Key::Space);
        h.run();
        assert!(h.query_by_role_and_label(Role::Button, "\"Pop Est\"").is_some(), "asked for");
    }

    /// Many fields: at most `MAX_FIELDS` of them, and the language's items still after them.
    #[test]
    fn many_fields_leave_room_for_the_language() {
        let fields = (0..80).map(|i| Field::new(format!("f{i:02}"), FieldType::Integer)).collect();
        let ds = Dataset::new("t", GeometryType::Point, fields);
        let o = offer_at("", 0, &ds, true).unwrap();
        assert_eq!(o.of.iter().filter(|i| matches!(i, Item::Field(_))).count(), MAX_FIELDS);
        for want in ["round()", "$area", "CASE"] {
            assert!(o.items.iter().any(|i| i == want), "{want}");
        }
    }

    /// What accepting writes, and where the cursor goes: `▮` marks it, in `text` (before) and in
    /// what's expected (after).
    #[test]
    fn accepting_writes_the_item_as_the_expression_needs_it() {
        let ds =
            Dataset::new("t", GeometryType::Point, vec![Field::new("Pop Est", FieldType::Integer), Field::new("Area!", FieldType::Double)]);
        for (text, item, want) in [
            ("Pop▮", "\"Pop Est\"", "\"Pop Est\"▮"),
            ("\"Pop▮\" > 1", "\"Pop Est\"", "\"Pop Est\"▮ > 1"),
            ("!Pop▮", "\"Pop Est\"", "!Pop Est!▮"),
            // `!` has no escape: a name holding one goes in double quotes.
            ("!Ar▮", "\"Area!\"", "\"Area!\"▮"),
            ("!Ar▮!", "\"Area!\"", "\"Area!\"▮"),
            // The rest of the word after the cursor is replaced too.
            ("Po▮p", "\"Pop Est\"", "\"Pop Est\"▮"),
            ("\"Po▮p E\" > 1", "\"Pop Est\"", "\"Pop Est\"▮ > 1"),
            ("\"Po▮p \"\"E\" > 1", "\"Pop Est\"", "\"Pop Est\"▮ > 1"),
            ("ro▮und(2)", "round()", "round(▮2)"),
            ("ro▮", "round()", "round(▮"),
            ("ro▮(2)", "round()", "round(▮2)"),
            ("p▮", "pi()", "pi()▮"),
        ] {
            let at = text.chars().position(|c| c == '▮').unwrap();
            let text = text.replace('▮', "");
            let offer = offer_at(&text, at, &ds, false).unwrap();
            let (new, cursor) = written(&text, &offer, item).unwrap();
            let mut got: Vec<char> = new.chars().collect();
            got.insert(cursor, '▮');
            assert_eq!(got.into_iter().collect::<String>(), want, "{text}");
        }
    }
}
