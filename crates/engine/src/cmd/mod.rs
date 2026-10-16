//! The drafting commands (commands.js `def(...)`): the registry, lookup by name or alias.

pub mod draw;
pub mod modify;
pub mod trim;
pub mod view;

use crate::engine::Ctx;
use crate::prompt::CmdFuture;

/// One command.
pub struct CmdDef {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    /// Enter at an empty command line does not repeat it
    pub no_repeat: bool,
    /// it leaves the selection as it was
    pub keep_selection: bool,
    pub run: fn(Ctx) -> CmdFuture,
}

const fn def(name: &'static str, aliases: &'static [&'static str], run: fn(Ctx) -> CmdFuture) -> CmdDef {
    CmdDef { name, aliases, no_repeat: false, keep_selection: false, run }
}

pub static COMMANDS: &[CmdDef] = &[
    def("LINE", &["L"], |c| Box::pin(draw::line(c))),
    def("PLINE", &["PL"], |c| Box::pin(draw::pline(c))),
    def("TTR", &[], |c| Box::pin(draw::circle_ttr_cmd(c))),
    def("CIRCLE", &["C"], |c| Box::pin(draw::circle(c))),
    def("ARC", &["A"], |c| Box::pin(draw::arc(c))),
    def("RECTANG", &["REC", "RECTANGLE"], |c| Box::pin(draw::rectang(c))),
    def("POLYGON", &["POL"], |c| Box::pin(draw::polygon(c))),
    def("ELLIPSE", &["EL"], |c| Box::pin(draw::ellipse(c))),
    def("POINT", &["PO"], |c| Box::pin(draw::point(c))),
    def("TEXT", &["DT", "DTEXT"], |c| Box::pin(draw::text(c))),
    def("ERASE", &["E", "DELETE"], |c| Box::pin(modify::erase(c))),
    def("MOVE", &["M"], |c| Box::pin(modify::move_like(c, false))),
    def("COPY", &["CO", "CP"], |c| Box::pin(modify::move_like(c, true))),
    def("ROTATE", &["RO"], |c| Box::pin(modify::rotate(c))),
    def("SCALE", &["SC"], |c| Box::pin(modify::scale(c))),
    def("MIRROR", &["MI"], |c| Box::pin(modify::mirror(c))),
    def("OFFSET", &["O"], |c| Box::pin(modify::offset(c))),
    def("TRIM", &["TR"], |c| Box::pin(trim::trim_or_extend(c, true))),
    def("EXTEND", &["EX"], |c| Box::pin(trim::trim_or_extend(c, false))),
    CmdDef { no_repeat: true, keep_selection: true, ..def("ZOOM", &["Z"], |c| Box::pin(view::zoom(c))) },
    CmdDef { no_repeat: true, ..def("UNDO", &["U"], |c| Box::pin(view::undo(c))) },
    CmdDef { no_repeat: true, ..def("REDO", &[], |c| Box::pin(view::redo(c))) },
];

/// A command by name or alias (case-insensitive; a leading `_ . ' -` is ignored).
pub fn lookup(name: &str) -> Option<&'static CmdDef> {
    let raw = cc_cad::js::trim(name).to_uppercase();
    if raw.starts_with(['-', '_'])
        && let Some(c) = COMMANDS.iter().find(|c| c.name == raw)
    {
        return Some(c);
    }
    let u = raw.trim_start_matches(['_', '.', '\'', '-']);
    COMMANDS.iter().find(|c| c.name == u).or_else(|| COMMANDS.iter().find(|c| c.aliases.contains(&u)))
}

/// The names (for the CLI and tests).
pub fn names() -> Vec<&'static str> {
    COMMANDS.iter().map(|c| c.name).collect()
}
