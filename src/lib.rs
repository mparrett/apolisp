//! apolisp — a small Lisp in the Clojure dialect with its own VM.
//!
//! One library file with inline `mod` blocks (ADR-015, ADR-031). The module
//! paths are the ones chosen up front, so extraction into files later is a move
//! rather than a redesign. The seams are for subtraction: each `mod` below
//! should be cuttable or liftable, not merely a home for code.
//!
//! `main.rs` is the process driver and holds no language behaviour, so the
//! reader, printer, and values can be tested as values rather than through a
//! subprocess (ADR-031).
//!
//! Milestone 1 (BUILD.md): reader + printer + forms with span origins.
// ---------------------------------------------------------------------------

/// Source positions and reader errors.
///
/// Spans are byte offsets into one source string; line and column are derived
/// only when an error is rendered. Storing both would be two things to keep in
/// agreement, and only one of them is needed on the hot path.
pub mod error {
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub struct Span {
        pub start: u32,
        pub end: u32,
    }

    impl Span {
        pub fn new(start: usize, end: usize) -> Span {
            Span {
                start: start as u32,
                end: end as u32,
            }
        }
    }

    /// Where a span came from (ADR-026).
    ///
    /// `Generated` carries the macro call site rather than a position in any
    /// file. `Unknown` prints as unknown — the point of naming it is that span
    /// loss stays visible instead of degrading into a plausible lie.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub enum SpanOrigin {
        Source(Span),
        Generated(Span),
        Unknown,
    }

    impl SpanOrigin {
        pub fn span(&self) -> Option<Span> {
            match self {
                SpanOrigin::Source(s) | SpanOrigin::Generated(s) => Some(*s),
                SpanOrigin::Unknown => None,
            }
        }
    }

    #[derive(Debug)]
    pub struct LispErr {
        pub msg: String,
        pub origin: SpanOrigin,
        /// The input ended *mid-form* rather than being wrong. Only the reader
        /// sets it and only a REPL asks: it is the difference between "keep
        /// typing" and "that is a syntax error" (ADR-044 part 5).
        ///
        /// A flag and not a test on `msg`, for ADR-039 clause 3's reason one
        /// layer down — prose is text this project reserves the right to
        /// reword, so nothing may depend on it.
        pub truncated: bool,
    }

    impl LispErr {
        pub fn at(span: Span, msg: impl Into<String>) -> LispErr {
            LispErr {
                msg: msg.into(),
                origin: SpanOrigin::Source(span),
                truncated: false,
            }
        }

        /// The same error, marked as "the input stopped early". Every caller is
        /// a reader path that ran out of bytes with a form still open.
        pub fn truncated(self) -> LispErr {
            LispErr {
                truncated: true,
                ..self
            }
        }

        /// For phases downstream of the reader, whose input may be generated
        /// rather than read. A compiler error on macro output has no file
        /// position, and inventing one is worse than saying so (ADR-026).
        pub fn at_origin(origin: SpanOrigin, msg: impl Into<String>) -> LispErr {
            LispErr {
                msg: msg.into(),
                origin,
                truncated: false,
            }
        }

        /// Render with 1-based line and column, resolved against the source
        /// the span came from.
        pub fn render(&self, path: &str, src: &str) -> String {
            match self.origin.span() {
                Some(s) => {
                    let (line, col) = line_col(src, s.start as usize);
                    format!("{path}:{line}:{col}: {}", self.msg)
                }
                None => format!("{path}: {}", self.msg),
            }
        }
    }

    pub fn line_col(src: &str, offset: usize) -> (usize, usize) {
        let mut line = 1;
        let mut line_start = 0;
        for (i, b) in src.bytes().enumerate() {
            if i >= offset {
                break;
            }
            if b == b'\n' {
                line += 1;
                line_start = i + 1;
            }
        }
        // Column counts characters, not bytes, so a multi-byte character does
        // not report a column past the end of its own line.
        let col = src[line_start..offset.min(src.len())].chars().count() + 1;
        (line, col)
    }
}

// ---------------------------------------------------------------------------

/// Values. Forms *are* values (ADR-023); what makes a value a form is the span
/// origins travelling beside it, not its representation.
pub mod value {
    use crate::error::SpanOrigin;
    use std::collections::HashMap;
    use std::rc::Rc;

    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
    pub struct SymId(pub u32);
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
    pub struct KwId(pub u32);
    // Arena indices, not pointers (ADR-025). Unused until their milestone, but
    // present so `size_of::<Value>()` is asserted against the real enum.
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
    pub struct CellId(pub u32, pub u32);
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
    pub struct HandleId(pub u32, pub u32);
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
    pub struct BufferId(pub u32, pub u32);

    // `Clone` is not decoration: ADR-041 builds collections with
    // `Rc::make_mut`, which mutates when the refcount is one and clones when it
    // is not. That is where the transient win comes from, and it is why these
    // derive `Clone` rather than being cloned by hand at each call site.
    #[derive(Clone, Debug)]
    pub struct StrObj(pub String);
    #[derive(Clone, Debug)]
    pub struct BytesObj(pub Vec<u8>);
    #[derive(Clone, Debug)]
    pub struct ListObj(pub Vec<Value>);
    #[derive(Clone, Debug)]
    pub struct VecObj(pub Vec<Value>);
    /// Insertion-ordered pairs. Q6 owns the real representation; this one is
    /// provisional and deliberately the dumbest thing that reads back in the
    /// order it was written, because iteration order reaches golden output and
    /// nondeterminism there kills the oracle (BUILD.md).
    #[derive(Clone, Debug)]
    pub struct MapObj(pub Vec<(Value, Value)>);

    /// An index into the VM's native function table (ADR-038).
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
    pub struct NativeId(pub u32);

    /// ADR-038. A native function is a kind of closure, so it is called through
    /// the ordinary `Call` and `Value` needs no new variant — ADR-025 stands.
    ///
    /// `proto` is a plain `u32` rather than `bytecode::ProtoIdx` because
    /// `bytecode` depends on this module and not the other way round. Captures
    /// are copied in at creation and never referenced (ADR-002).
    #[derive(Debug)]
    pub enum Closure {
        Fn { proto: u32, captures: Rc<[Value]> },
        Native(NativeId),
    }

    /// ADR-025. `PartialEq` is implemented by hand below, never derived —
    /// derived equality follows Rust variant and payload equality, which is the
    /// wrong answer for a language whose `=` crosses representations
    /// (`TRAPS.md`).
    #[derive(Clone, Debug)]
    pub enum Value {
        Nil,
        Bool(bool),
        Int(i64),
        Float(f64),
        Str(Rc<StrObj>),
        Bytes(Rc<BytesObj>),
        Sym(SymId),
        Keyword(KwId),
        List(Rc<ListObj>),
        Vec(Rc<VecObj>),
        Map(Rc<MapObj>),
        Fn(Rc<Closure>),
        Cell(CellId),
        Handle(HandleId),
        Buffer(BufferId),
    }

    impl PartialEq for Value {
        fn eq(&self, other: &Value) -> bool {
            use Value::*;
            match (self, other) {
                (Nil, Nil) => true,
                (Bool(a), Bool(b)) => a == b,
                (Int(a), Int(b)) => a == b,
                // Q13 owns numeric equality: whether 1 == 1.0, and NaN/-0.0.
                // Until it is decided, floats compare by IEEE rules and never
                // to integers, which is the option that is easy to widen later.
                (Float(a), Float(b)) => a == b,
                (Str(a), Str(b)) => a.0 == b.0,
                (Bytes(a), Bytes(b)) => a.0 == b.0,
                (Sym(a), Sym(b)) => a == b,
                (Keyword(a), Keyword(b)) => a == b,
                (List(a), List(b)) => a.0 == b.0,
                (Vec(a), Vec(b)) => a.0 == b.0,
                (Map(a), Map(b)) => a.0 == b.0,
                // Q20: Clojure makes '(1 2) equal [1 2]. Cross-type sequential
                // equality is deliberately absent until that question is
                // settled — widening equality later is safe, narrowing is not.
                _ => false,
            }
        }
    }

    /// Language equality (ADR-041 part 2): structural, crossing list and
    /// vector, never crossing `Int` and `Float`.
    ///
    /// Deliberately not Rust's `PartialEq` on `Value`, which is the wrong
    /// answer for the language and is kept for what it is right for — constant
    /// pool deduplication, where `1` and `1.0` must stay distinct entries
    /// (`TRAPS.md`).
    pub fn equal(a: &Value, b: &Value) -> bool {
        match (a, b) {
            (Value::Nil, Value::Nil) => true,
            (Value::Bool(x), Value::Bool(y)) => x == y,
            (Value::Int(x), Value::Int(y)) => x == y,
            // IEEE, so `##NaN` equals nothing and `-0.0` equals `0.0`. Compared
            // as numbers rather than bit patterns, which is the difference
            // between this and the constant pool's rule.
            (Value::Float(x), Value::Float(y)) => x == y,
            (Value::Str(x), Value::Str(y)) => x.0 == y.0,
            (Value::Bytes(x), Value::Bytes(y)) => x.0 == y.0,
            (Value::Sym(x), Value::Sym(y)) => x == y,
            (Value::Keyword(x), Value::Keyword(y)) => x == y,
            // One abstraction, two representations.
            (Value::List(x), Value::List(y)) => seq_equal(&x.0, &y.0),
            (Value::Vec(x), Value::Vec(y)) => seq_equal(&x.0, &y.0),
            (Value::List(x), Value::Vec(y)) | (Value::Vec(y), Value::List(x)) => {
                seq_equal(&x.0, &y.0)
            }
            (Value::Map(x), Value::Map(y)) => map_equal(&x.0, &y.0),
            // No structure to compare: these are identities.
            (Value::Fn(x), Value::Fn(y)) => Rc::ptr_eq(x, y),
            (Value::Cell(x), Value::Cell(y)) => x == y,
            (Value::Handle(x), Value::Handle(y)) => x == y,
            (Value::Buffer(x), Value::Buffer(y)) => x == y,
            _ => false,
        }
    }

    fn seq_equal(a: &[Value], b: &[Value]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| equal(x, y))
    }

    /// Insertion order is not part of a map's identity, so this is a
    /// membership check both ways rather than a zip. Quadratic, and the size
    /// where that matters is the size where the representation itself is the
    /// problem (ADR-041's cost clause).
    fn map_equal(a: &[(Value, Value)], b: &[(Value, Value)]) -> bool {
        a.len() == b.len()
            && a.iter().all(|(k, v)| {
                b.iter()
                    .find(|(k2, _)| equal(k, k2))
                    .is_some_and(|(_, v2)| equal(v, v2))
            })
    }

    /// Identity for the *constant pool*, which is a different question from
    /// language `=` (ADR-041 part 2) and from Rust's derived `PartialEq`.
    ///
    /// Two constants may share a pool entry only if no program can tell them
    /// apart. Floats therefore compare by **bit pattern**: `-0.0` and `0.0` are
    /// equal as numbers and are not the same constant, and merging them makes
    /// `(/ 1.0 0.0)` produce `##-Inf` in a chunk that mentioned `-0.0`
    /// earlier — a miscompilation with no diagnostic, which is what the
    /// in-language suite caught. `##NaN` merges with itself by the same rule,
    /// which is safe for the opposite reason: identical bits, no observable
    /// difference.
    pub fn same_const(a: &Value, b: &Value) -> bool {
        match (a, b) {
            (Value::Float(x), Value::Float(y)) => x.to_bits() == y.to_bits(),
            // Never across representations: a list and a vector are equal to
            // the language and are still two different constants.
            (Value::List(x), Value::List(y)) => same_consts(&x.0, &y.0),
            (Value::Vec(x), Value::Vec(y)) => same_consts(&x.0, &y.0),
            (Value::Map(x), Value::Map(y)) => {
                x.0.len() == y.0.len()
                    && x.0
                        .iter()
                        .zip(&y.0)
                        .all(|((k1, v1), (k2, v2))| same_const(k1, k2) && same_const(v1, v2))
            }
            _ => a == b,
        }
    }

    fn same_consts(a: &[Value], b: &[Value]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| same_const(x, y))
    }

    /// One table for symbols and keywords. They are distinct `Value` variants
    /// (ADR-025) but share interning, so an id alone does not say which kind
    /// you have — that is a `TRAPS.md` entry, and the reason the variants are
    /// separate rather than a flag bit.
    #[derive(Debug)]
    pub struct Interner {
        names: Vec<String>,
        index: HashMap<String, u32>,
    }

    impl Interner {
        pub fn new() -> Interner {
            Interner {
                names: Vec::new(),
                index: HashMap::new(),
            }
        }

        /// Positional, so the whole table travels as its names and the index
        /// is rebuilt (ADR-043 part 6). A snapshot that omitted it would resume
        /// with wrong symbol identities and *appear to work*, which `TRAPS.md`
        /// lists as the dangerous one.
        pub fn names(&self) -> &[String] {
            &self.names
        }

        pub fn restore(names: Vec<String>) -> Interner {
            let index = names
                .iter()
                .enumerate()
                .map(|(i, n)| (n.clone(), i as u32))
                .collect();
            Interner { names, index }
        }

        pub fn intern(&mut self, name: &str) -> u32 {
            if let Some(&id) = self.index.get(name) {
                return id;
            }
            let id = self.names.len() as u32;
            self.names.push(name.to_string());
            self.index.insert(name.to_string(), id);
            id
        }

        pub fn sym(&mut self, name: &str) -> Value {
            Value::Sym(SymId(self.intern(name)))
        }

        pub fn keyword(&mut self, name: &str) -> Value {
            Value::Keyword(KwId(self.intern(name)))
        }

        pub fn name(&self, id: u32) -> &str {
            &self.names[id as usize]
        }
    }

    impl Default for Interner {
        fn default() -> Interner {
            Interner::new()
        }
    }

    /// The span carrier (ADR-026). Origins live outside the value graph and
    /// mirror its shape: one entry per syntactic child, so immediates are
    /// covered by the same mechanism as heap objects.
    ///
    /// A map contributes two children per pair, key then value.
    #[derive(Clone, Debug)]
    pub struct Origins {
        pub origin: SpanOrigin,
        pub children: Vec<Origins>,
    }

    impl Origins {
        pub fn leaf(origin: SpanOrigin) -> Origins {
            Origins {
                origin,
                children: Vec::new(),
            }
        }
    }

    /// A value the reader produced, or that the expander is treating as code.
    #[derive(Clone, Debug)]
    pub struct LocatedForm {
        pub root: Value,
        pub origins: Origins,
    }

    /// How many syntactic children a value has, for checking that an `Origins`
    /// tree still lines up with the value it describes.
    pub fn child_count(v: &Value) -> usize {
        match v {
            Value::List(l) => l.0.len(),
            Value::Vec(v) => v.0.len(),
            Value::Map(m) => m.0.len() * 2,
            _ => 0,
        }
    }

    /// Walk a value against its origins and collect every way they disagree.
    ///
    /// Three invariants (ADR-026): a `Source` span lies inside the file it came
    /// from, both of its ends fall on character boundaries, and child-origin
    /// arity matches child count. The arity check is the one that catches a
    /// whole *category* of node silently having no origin, which is the failure
    /// `../reg-lisp` hit and its suite missed.
    ///
    /// The boundary check earns its place because the alternative to reporting
    /// a misaligned span is panicking inside error rendering — a reader bug
    /// then presents as a crash on the input it was meant to reject.
    pub fn check_origins(v: &Value, o: &Origins, src: &str, out: &mut Vec<String>) {
        if let SpanOrigin::Source(s) = o.origin {
            let (start, end) = (s.start as usize, s.end as usize);
            if s.start > s.end || end > src.len() {
                out.push(format!(
                    "span {}..{} outside source of {} bytes",
                    s.start,
                    s.end,
                    src.len()
                ));
            } else if !src.is_char_boundary(start) || !src.is_char_boundary(end) {
                out.push(format!(
                    "span {}..{} does not lie on character boundaries",
                    s.start, s.end
                ));
            }
        }
        let want = child_count(v);
        if o.children.len() != want {
            out.push(format!(
                "{} has {want} syntactic children but {} origins",
                kind_name(v),
                o.children.len()
            ));
            return; // Arity is already wrong; recursing would report noise.
        }
        for (child, co) in children(v).iter().zip(&o.children) {
            check_origins(child, co, src, out);
        }
    }

    /// Syntactic children in source order. A map contributes key then value.
    pub fn children(v: &Value) -> Vec<Value> {
        match v {
            Value::List(l) => l.0.clone(),
            Value::Vec(x) => x.0.clone(),
            Value::Map(m) => {
                m.0.iter()
                    .flat_map(|(k, v)| [k.clone(), v.clone()])
                    .collect()
            }
            _ => Vec::new(),
        }
    }

    pub fn kind_name(v: &Value) -> &'static str {
        match v {
            Value::Nil => "nil",
            Value::Bool(_) => "bool",
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Str(_) => "string",
            Value::Bytes(_) => "bytes",
            Value::Sym(_) => "symbol",
            Value::Keyword(_) => "keyword",
            Value::List(_) => "list",
            Value::Vec(_) => "vector",
            Value::Map(_) => "map",
            Value::Fn(_) => "fn",
            Value::Cell(_) => "cell",
            Value::Handle(_) => "handle",
            Value::Buffer(_) => "buffer",
        }
    }

    /// The debug rendering ADR-026 calls for. Origins are invisible in ordinary
    /// printed output by construction, so without this the invariants could
    /// only be inspected through a debugger.
    pub fn print_origins(v: &Value, o: &Origins, interner: &Interner, depth: usize) -> String {
        let pad = "  ".repeat(depth);
        let loc = match o.origin {
            SpanOrigin::Source(s) => format!("{}..{}", s.start, s.end),
            SpanOrigin::Generated(s) => format!("generated@{}..{}", s.start, s.end),
            SpanOrigin::Unknown => "unknown".to_string(),
        };
        let mut out = format!(
            "{pad}{} {} {}",
            loc,
            kind_name(v),
            crate::printer::print(v, interner)
        );
        for (child, co) in children(v).iter().zip(&o.children) {
            out.push('\n');
            out.push_str(&print_origins(child, co, interner, depth + 1));
        }
        out
    }

    /// ADR-025: the size is asserted, not assumed. The limit and the
    /// measurement live here, next to the type; printing them is the driver's
    /// job, and a test can now assert the number rather than parse a report.
    pub const VALUE_SIZE_LIMIT: usize = 24;

    pub fn value_size() -> usize {
        std::mem::size_of::<Value>()
    }

    pub fn origins_size() -> usize {
        std::mem::size_of::<Origins>()
    }
}

// ---------------------------------------------------------------------------

/// The reader. Character-driven and producing data, not a grammar-shaped tree —
/// which is why there is no separate tokenizer to fight reader-macro dispatch
/// later (ADR-014).
pub mod reader {
    use crate::error::{LispErr, Span, SpanOrigin};
    use crate::value::{
        equal, Interner, ListObj, LocatedForm, MapObj, Origins, StrObj, Value, VecObj,
    };
    use std::rc::Rc;

    /// ADR-036. Every phase in front of the VM recurses on the host stack — the
    /// reader, the origin walkers, `Rc`'s drop glue over a nested list, the
    /// resolver, the lowering — and each of them recurses at most once per level
    /// of form nesting. So bounding depth here bounds all of them, and this is
    /// the only place that checks.
    ///
    /// Sized for the smallest stack this runs on, which is a 2 MB cargo test
    /// thread rather than the 8 MB main thread.
    pub const MAX_NESTING: usize = 256;

    pub fn read_all(src: &str, interner: &mut Interner) -> Result<Vec<LocatedForm>, LispErr> {
        let mut r = Reader {
            src,
            pos: 0,
            depth: 0,
            interner,
        };
        let mut out = Vec::new();
        loop {
            r.skip_ws();
            if r.at_end() {
                return Ok(out);
            }
            out.push(r.read_form()?);
        }
    }

    struct Reader<'a> {
        src: &'a str,
        pos: usize,
        depth: usize,
        interner: &'a mut Interner,
    }

    impl<'a> Reader<'a> {
        fn at_end(&self) -> bool {
            self.pos >= self.src.len()
        }

        fn peek(&self) -> Option<u8> {
            self.src.as_bytes().get(self.pos).copied()
        }

        fn skip_ws(&mut self) {
            loop {
                match self.peek() {
                    // Commas are whitespace, as in Clojure.
                    Some(b) if b.is_ascii_whitespace() || b == b',' => {
                        self.pos += 1;
                    }
                    Some(b';') => {
                        while let Some(b) = self.peek() {
                            if b == b'\n' {
                                break;
                            }
                            self.pos += 1;
                        }
                    }
                    _ => return,
                }
            }
        }

        fn err_here(&self, msg: impl Into<String>) -> LispErr {
            LispErr::at(Span::new(self.pos, self.pos), msg)
        }

        /// The depth bound (ADR-036) lives here rather than in each recursive
        /// case, because this is the single point every one of them goes
        /// through. Without it a deeply nested file overflows the host stack and
        /// the process is killed, which is a stack overflow presenting as a
        /// crash rather than as a diagnostic.
        fn read_form(&mut self) -> Result<LocatedForm, LispErr> {
            if self.depth >= MAX_NESTING {
                self.skip_ws();
                return Err(self.err_here(format!("nested more than {MAX_NESTING} deep (ADR-036)")));
            }
            self.depth += 1;
            let form = self.read_form_inner();
            self.depth -= 1;
            form
        }

        fn read_form_inner(&mut self) -> Result<LocatedForm, LispErr> {
            self.skip_ws();
            let start = self.pos;
            let b = match self.peek() {
                Some(b) => b,
                None => return Err(self.err_here("unexpected end of input").truncated()),
            };

            match b {
                b'(' => self.read_seq(b')', start, |items| Value::List(Rc::new(ListObj(items)))),
                b'[' => self.read_seq(b']', start, |items| Value::Vec(Rc::new(VecObj(items)))),
                b'{' => self.read_map(start),
                b')' | b']' | b'}' => {
                    self.pos += 1;
                    Err(LispErr::at(
                        Span::new(start, self.pos),
                        format!("unmatched `{}`", b as char),
                    ))
                }
                b'"' => self.read_string(start),
                // The four prefix sugars, all the same shape: `(name form)`.
                // Quasiquote is *not* expanded here — the reader desugars the
                // punctuation and the expander lowers the template, so each
                // phase's golden shows its own job (ADR-039's sibling reasoning
                // in ADR-040).
                b'\'' => self.read_prefixed("quote", 1, start),
                b'`' => self.read_prefixed("quasiquote", 1, start),
                b'~' if self.src.as_bytes().get(start + 1) == Some(&b'@') => {
                    self.read_prefixed("unquote-splicing", 2, start)
                }
                b'~' => self.read_prefixed("unquote", 1, start),
                b':' => self.read_keyword(start),
                _ => self.read_atom(start),
            }
        }

        /// `'x` reads as `(quote x)`, and the other three sugars the same way.
        /// The synthesized head symbol has no source text of its own, so it
        /// takes the span of the punctuation that produced it rather than
        /// claiming a position it does not occupy.
        fn read_prefixed(
            &mut self,
            name: &str,
            len: usize,
            start: usize,
        ) -> Result<LocatedForm, LispErr> {
            self.pos += len;
            let head = self.interner.sym(name);
            let inner = self.read_form()?;
            Ok(LocatedForm {
                root: Value::List(Rc::new(ListObj(vec![head, inner.root]))),
                origins: Origins {
                    origin: SpanOrigin::Source(Span::new(start, self.pos)),
                    children: vec![
                        Origins::leaf(SpanOrigin::Source(Span::new(start, start + len))),
                        inner.origins,
                    ],
                },
            })
        }

        fn read_seq(
            &mut self,
            close: u8,
            start: usize,
            build: impl Fn(Vec<Value>) -> Value,
        ) -> Result<LocatedForm, LispErr> {
            self.pos += 1; // opening delimiter
            let mut items = Vec::new();
            let mut origins = Vec::new();
            loop {
                self.skip_ws();
                match self.peek() {
                    None => {
                        // Report the opener, not the end of the file: in a long
                        // file the unclosed delimiter is the only position that
                        // tells you anything.
                        return Err(LispErr::at(
                            Span::new(start, start + 1),
                            format!("unclosed `{}`", self.src.as_bytes()[start] as char),
                        )
                        .truncated());
                    }
                    Some(b) if b == close => {
                        self.pos += 1;
                        let span = Span::new(start, self.pos);
                        return Ok(LocatedForm {
                            root: build(items),
                            origins: Origins {
                                origin: SpanOrigin::Source(span),
                                children: origins,
                            },
                        });
                    }
                    _ => {
                        let f = self.read_form()?;
                        items.push(f.root);
                        origins.push(f.origins);
                    }
                }
            }
        }

        fn read_map(&mut self, start: usize) -> Result<LocatedForm, LispErr> {
            self.pos += 1;
            let mut pairs: Vec<(Value, Value)> = Vec::new();
            let mut origins = Vec::new();
            loop {
                self.skip_ws();
                match self.peek() {
                    None => {
                        return Err(
                            LispErr::at(Span::new(start, start + 1), "unclosed `{`").truncated()
                        );
                    }
                    Some(b'}') => {
                        self.pos += 1;
                        let span = Span::new(start, self.pos);
                        return Ok(LocatedForm {
                            root: Value::Map(Rc::new(MapObj(pairs))),
                            origins: Origins {
                                origin: SpanOrigin::Source(span),
                                children: origins,
                            },
                        });
                    }
                    _ => {
                        let k = self.read_form()?;
                        self.skip_ws();
                        // Running out of input is an unclosed brace, not an odd
                        // key count. Both are true of `{:a`, but only one of
                        // them tells you what to type next.
                        match self.peek() {
                            None => {
                                return Err(LispErr::at(
                                    Span::new(start, start + 1),
                                    "unclosed `{`",
                                )
                                .truncated())
                            }
                            Some(b'}') => {
                                return Err(LispErr::at(
                                    k.origins
                                        .origin
                                        .span()
                                        .unwrap_or(Span::new(start, start + 1)),
                                    "map literal has a key with no value",
                                ))
                            }
                            _ => {}
                        }
                        let v = self.read_form()?;
                        // Last write wins, so a map never holds two equal
                        // keys (ADR-041 part 4) — including a map literal,
                        // which is construction like any other. The surviving
                        // *key* is the first occurrence, so its position is
                        // where the map says it came from; only the value and
                        // its origin are replaced, which keeps origins paired
                        // with the pairs they describe (ADR-026).
                        match pairs
                            .iter()
                            .position(|(existing, _)| equal(existing, &k.root))
                        {
                            Some(i) => {
                                pairs[i].1 = v.root;
                                origins[i * 2 + 1] = v.origins;
                            }
                            None => {
                                pairs.push((k.root, v.root));
                                origins.push(k.origins);
                                origins.push(v.origins);
                            }
                        }
                    }
                }
            }
        }

        fn read_string(&mut self, start: usize) -> Result<LocatedForm, LispErr> {
            self.pos += 1;
            let mut s = String::new();
            loop {
                let b = match self.peek() {
                    None => {
                        return Err(LispErr::at(
                            Span::new(start, start + 1),
                            "unclosed string literal",
                        )
                        .truncated())
                    }
                    Some(b) => b,
                };
                match b {
                    b'"' => {
                        self.pos += 1;
                        return Ok(self
                            .located(Value::Str(Rc::new(StrObj(s))), Span::new(start, self.pos)));
                    }
                    b'\\' => {
                        // The backslash offset is captured before advancing.
                        // The escaped character may be multi-byte, so the span
                        // cannot be recovered afterwards by subtracting a byte
                        // count — that lands inside a character and panics the
                        // renderer on input the reader is supposed to reject.
                        let esc_start = self.pos;
                        self.pos += 1;
                        let esc = match self.src[self.pos..].chars().next() {
                            None => return Err(self.err_here("unfinished escape sequence")),
                            Some(c) => c,
                        };
                        self.pos += esc.len_utf8();
                        match esc {
                            'n' => s.push('\n'),
                            't' => s.push('\t'),
                            'r' => s.push('\r'),
                            '\\' => s.push('\\'),
                            '"' => s.push('"'),
                            // Reported as a character, not as the first byte of
                            // one: `\€` names the escape the source actually
                            // wrote rather than a mojibake prefix of it.
                            other => {
                                return Err(LispErr::at(
                                    Span::new(esc_start, self.pos),
                                    format!("unknown escape `\\{other}`"),
                                ))
                            }
                        }
                    }
                    _ => {
                        let rest = &self.src[self.pos..];
                        let c = rest.chars().next().unwrap();
                        s.push(c);
                        self.pos += c.len_utf8();
                    }
                }
            }
        }

        fn read_keyword(&mut self, start: usize) -> Result<LocatedForm, LispErr> {
            self.pos += 1; // ':'
            let text_start = self.pos;
            self.scan_token();
            if self.pos == text_start {
                return Err(LispErr::at(Span::new(start, self.pos), "`:` with no name"));
            }
            let name = self.src[text_start..self.pos].to_string();
            let v = self.interner.keyword(&name);
            Ok(self.located(v, Span::new(start, self.pos)))
        }

        fn read_atom(&mut self, start: usize) -> Result<LocatedForm, LispErr> {
            self.scan_token();
            let text = &self.src[start..self.pos];
            if text.is_empty() {
                let c = self.peek().unwrap_or(b' ') as char;
                self.pos += 1;
                return Err(LispErr::at(
                    Span::new(start, self.pos),
                    format!("unexpected `{c}`"),
                ));
            }
            let span = Span::new(start, self.pos);
            let v = match text {
                "nil" => Value::Nil,
                "true" => Value::Bool(true),
                "false" => Value::Bool(false),
                // The non-finite spellings used to be matched here, ahead of
                // `parse_number`. They moved into it (ADR-046) so that the
                // number grammar is in one function rather than in one function
                // plus three arms of this match — which is what made
                // `(parse-number "##Inf")` answer `nil` while the reader read
                // the same three characters as a float.
                _ => match parse_number(text) {
                    Some(Ok(v)) => v,
                    Some(Err(why)) => return Err(LispErr::at(span, why)),
                    None => self.interner.sym(text),
                },
            };
            Ok(self.located(v, span))
        }

        fn scan_token(&mut self) {
            while let Some(b) = self.peek() {
                if b.is_ascii_whitespace()
                    || matches!(
                        b,
                        b'(' | b')' | b'[' | b']' | b'{' | b'}' | b'"' | b';' | b',' | b'`' | b'~'
                    )
                {
                    break;
                }
                self.pos += 1;
            }
        }

        fn located(&self, root: Value, span: Span) -> LocatedForm {
            LocatedForm {
                root,
                origins: Origins::leaf(SpanOrigin::Source(span)),
            }
        }
    }

    /// A token is a number only if the whole token parses. `1abc` is a symbol,
    /// not the integer 1 followed by junk — a partial parse would silently
    /// accept typos.
    ///
    /// `None` means "not numeric-looking, treat as a symbol". `Some(Err)` means
    /// it looked like a number and was not one; that is an error rather than a
    /// symbol, because an integer too large for `i64` reaching the compiler as
    /// a variable name is a silent wrong answer.
    /// Public because the `parse-number` primitive is this function (ADR-046).
    /// The grammar a literal is read by and the grammar a string is parsed by
    /// are one implementation, so they cannot drift.
    pub fn parse_number(text: &str) -> Option<Result<Value, String>> {
        // Clojure's spellings, and exactly what the printer emits. A printer
        // that emits tokens its own reader cannot read is a round-trip hole:
        // `##Inf` read back as a symbol prints as `##Inf` again, so
        // string-level comparison never notices the type change (ADR-032).
        //
        // They are here rather than in the caller so that `print` and
        // `parse-number` are inverses over every number, non-finite ones
        // included.
        match text {
            "##Inf" => return Some(Ok(Value::Float(f64::INFINITY))),
            "##-Inf" => return Some(Ok(Value::Float(f64::NEG_INFINITY))),
            "##NaN" => return Some(Ok(Value::Float(f64::NAN))),
            _ => {}
        }
        let looks_numeric = {
            let mut cs = text.chars();
            match cs.next() {
                Some(c) if c.is_ascii_digit() => true,
                Some('-') | Some('+') => cs.next().is_some_and(|c| c.is_ascii_digit()),
                _ => false,
            }
        };
        if !looks_numeric {
            return None;
        }
        if let Ok(i) = text.parse::<i64>() {
            return Some(Ok(Value::Int(i)));
        }
        // An integer literal too large for i64 is rejected rather than widened
        // to a float, which would lose digits without saying so. Q10 owns
        // overflow at runtime; this is the literal, which is decidable here.
        if !text.contains(['.', 'e', 'E']) {
            // Two ways to land here and they are not the same mistake: `1abc`
            // is a typo, and `99999999999999999999` is a number this machine
            // cannot hold. Reporting the first as an overflow sent people
            // looking for a range problem in a token that has letters in it.
            let digits = text.trim_start_matches(['-', '+']);
            return Some(Err(if digits.chars().all(|c| c.is_ascii_digit()) {
                format!("number `{text}` does not fit in a 64-bit integer")
            } else {
                format!("`{text}` is not a valid number")
            }));
        }
        if let Ok(f) = text.parse::<f64>() {
            // A finite-looking literal that overflows the range is rejected for
            // the same reason an oversized integer is: `1e400` becoming
            // infinity is a silent wrong answer, and the digits the source
            // wrote are gone. `##Inf` says it on purpose (ADR-032).
            //
            // Underflow is deliberately not symmetric — `1e-400` reads as zero,
            // as it does everywhere else. Losing precision near zero is the
            // ordinary condition of floating point; losing the value entirely
            // is not.
            if f.is_infinite() {
                return Some(Err(format!(
                    "number `{text}` overflows to infinity; write `##Inf` to mean it"
                )));
            }
            return Some(Ok(Value::Float(f)));
        }
        Some(Err(format!("`{text}` is not a valid number")))
    }
}

// ---------------------------------------------------------------------------

/// The printer. Paired with the reader by the round-trip property (BUILD.md).
pub mod printer {
    use crate::value::{Interner, Value};

    pub fn print(v: &Value, interner: &Interner) -> String {
        let mut out = String::new();
        write(&mut out, v, interner);
        out
    }

    /// What `println` emits: a string is its own characters, not a readable
    /// literal. Clojure draws the same line between `pr` and `print`, and the
    /// round-trip property is about `print` — this one deliberately does not
    /// read back.
    ///
    /// Only the top level is affected. A string *inside* a collection still
    /// prints readably, because the alternative renders `["a b" "c"]`
    /// indistinguishable from `["a" "b" "c"]`.
    pub fn display(v: &Value, interner: &Interner) -> String {
        match v {
            Value::Str(s) => s.0.clone(),
            _ => print(v, interner),
        }
    }

    fn write(out: &mut String, v: &Value, interner: &Interner) {
        match v {
            Value::Nil => out.push_str("nil"),
            Value::Bool(true) => out.push_str("true"),
            Value::Bool(false) => out.push_str("false"),
            Value::Int(i) => out.push_str(&i.to_string()),
            Value::Float(f) => out.push_str(&print_float(*f)),
            Value::Str(s) => write_string(out, &s.0),
            Value::Bytes(b) => {
                out.push_str("#bytes[");
                for (i, byte) in b.0.iter().enumerate() {
                    if i > 0 {
                        out.push(' ');
                    }
                    out.push_str(&byte.to_string());
                }
                out.push(']');
            }
            Value::Sym(s) => out.push_str(interner.name(s.0)),
            Value::Keyword(k) => {
                out.push(':');
                out.push_str(interner.name(k.0));
            }
            Value::List(l) => write_seq(out, &l.0, '(', ')', interner),
            Value::Vec(v) => write_seq(out, &v.0, '[', ']', interner),
            Value::Map(m) => {
                out.push('{');
                for (i, (k, val)) in m.0.iter().enumerate() {
                    if i > 0 {
                        out.push(' ');
                    }
                    write(out, k, interner);
                    out.push(' ');
                    write(out, val, interner);
                }
                out.push('}');
            }
            // These have no reader syntax. They print as opaque rather than as
            // something that would read back as a different value.
            Value::Fn(_) => out.push_str("#<fn>"),
            Value::Cell(c) => out.push_str(&format!("#<cell {}:{}>", c.0, c.1)),
            Value::Handle(h) => out.push_str(&format!("#<handle {}:{}>", h.0, h.1)),
            Value::Buffer(b) => out.push_str(&format!("#<buffer {}:{}>", b.0, b.1)),
        }
    }

    fn write_seq(out: &mut String, items: &[Value], open: char, close: char, interner: &Interner) {
        out.push(open);
        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                out.push(' ');
            }
            write(out, item, interner);
        }
        out.push(close);
    }

    fn write_string(out: &mut String, s: &str) {
        out.push('"');
        for c in s.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\t' => out.push_str("\\t"),
                '\r' => out.push_str("\\r"),
                _ => out.push(c),
            }
        }
        out.push('"');
    }

    /// Floats must print in a form that reads back as a float. Rust's default
    /// renders 1.0 as "1", which would read back as an integer and quietly
    /// break the round-trip property.
    fn print_float(f: f64) -> String {
        if f.is_nan() {
            return "##NaN".to_string();
        }
        if f.is_infinite() {
            return if f > 0.0 {
                "##Inf".into()
            } else {
                "##-Inf".into()
            };
        }
        let s = f.to_string();
        if s.contains('.') || s.contains('e') || s.contains('E') {
            s
        } else {
            format!("{s}.0")
        }
    }
}

// ---------------------------------------------------------------------------

/// The instruction set, the compiled function, and the disassembler (ADR-034).
///
/// Instructions are a typed enum with `u32` operands rather than packed words.
/// E-5 corrected ADR-006's claim that monotonic slots avoid a wider encoding —
/// no reuse raises the maximum live slot index, so a packed format's operand
/// fields are exactly what comes under pressure. Unpacked, there is no width
/// left to run out of.
pub mod bytecode {
    use crate::error::{line_col, SpanOrigin};
    use crate::printer;
    use crate::value::{Interner, SymId, Value};

    pub type Slot = u32;
    pub type ConstIdx = u32;
    pub type Pc = u32;
    pub type ProtoIdx = u32;
    pub type CaptureIdx = u32;

    /// A call's callee sits at `base` and its arguments at `base+1 ..= base+argc`
    /// — where left-to-right evaluation into monotonically allocated slots puts
    /// them anyway (ADR-033). The callee's frame receives the arguments in its
    /// own slots `0..argc`, so parameters are the first slots of a frame by
    /// construction.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub enum Instr {
        Const {
            dst: Slot,
            k: ConstIdx,
        },
        Move {
            dst: Slot,
            src: Slot,
        },
        /// The operand is the interned name, not a `CellId`: forward references
        /// then need no fixup, and the disassembly does not depend on the order
        /// globals were defined in (ADR-034). The VM still owns the cell
        /// (ADR-027).
        GetGlobal {
            dst: Slot,
            name: SymId,
        },
        SetGlobal {
            name: SymId,
            src: Slot,
        },
        GetCapture {
            dst: Slot,
            idx: CaptureIdx,
        },
        /// ADR-002: self-recursion resolves through the running closure's own
        /// identity, never through a capture.
        GetSelf {
            dst: Slot,
        },
        SetCell {
            cell: Slot,
            src: Slot,
        },
        Closure {
            dst: Slot,
            proto: ProtoIdx,
        },
        Call {
            dst: Slot,
            base: Slot,
            argc: u32,
        },
        TailCall {
            base: Slot,
            argc: u32,
        },
        Return {
            src: Slot,
        },
        Jump {
            target: Pc,
        },
        /// Only `nil` and `false` are falsy (`TRAPS.md`).
        JumpUnless {
            cond: Slot,
            target: Pc,
        },
        Throw {
            src: Slot,
        },
        PushHandler {
            catch: Pc,
            err: Slot,
        },
        PushFinally {
            target: Pc,
        },
        PopHandler,
        EndFinally,
    }

    /// ADR-034 asserts this the way ADR-025 asserts `Value`'s: the number is
    /// measured, not assumed. The point of not packing was to stop paying
    /// attention to operand widths, and the assertion is what keeps that from
    /// becoming an excuse for an instruction that carries a `String`.
    pub const INSTR_SIZE_LIMIT: usize = 16;

    pub fn instr_size() -> usize {
        std::mem::size_of::<Instr>()
    }

    /// Where a capture's value comes from, read in the **enclosing** frame at
    /// the moment `Closure` runs. Copied, not referenced — that is the whole of
    /// ADR-002.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub enum CaptureSrc {
        Local(Slot),
        Capture(CaptureIdx),
        SelfFn,
    }

    /// One compiled function. A *prototype* rather than a function because it
    /// holds no captured values: a closure is this plus the values captured at
    /// the moment it was created.
    #[derive(Debug)]
    pub struct Proto {
        pub name: Option<SymId>,
        /// Named parameters, including the rest parameter when `variadic`.
        pub params: u32,
        pub variadic: bool,
        pub slots: u32,
        pub code: Vec<Instr>,
        /// ADR-023 point 2: `lines[i]` is the origin of the instruction at `i`.
        /// Kept parallel to `code` by construction — every instruction is
        /// emitted through one function that pushes to both.
        pub lines: Vec<SpanOrigin>,
        pub consts: Vec<Value>,
        pub captures: Vec<CaptureSrc>,
    }

    /// What compiling one file produces: `protos[0]` is the top level, and
    /// every nested `fn` is an index into this vector, reserved in source order
    /// so the disassembly is stable (ADR-034).
    #[derive(Debug)]
    pub struct Chunk {
        pub protos: Vec<Proto>,
    }

    pub fn disassemble(chunk: &Chunk, interner: &Interner, src: &str) -> String {
        let mut out = String::new();
        for (i, p) in chunk.protos.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            disasm_proto(&mut out, i, p, interner, src);
        }
        out
    }

    fn disasm_proto(out: &mut String, idx: usize, p: &Proto, interner: &Interner, src: &str) {
        let name = match (p.name, idx) {
            (Some(s), _) => interner.name(s.0),
            (None, 0) => "<top>",
            (None, _) => "<fn>",
        };
        let params = if p.variadic {
            format!("{} &rest", p.params - 1)
        } else {
            p.params.to_string()
        };
        out.push_str(&format!(
            "proto {idx}  {name}  params {params}  slots {}\n",
            p.slots
        ));
        if !p.consts.is_empty() {
            out.push_str("  constants\n");
            for (k, v) in p.consts.iter().enumerate() {
                out.push_str(&format!("    k{k}  {}\n", printer::print(v, interner)));
            }
        }
        if !p.captures.is_empty() {
            out.push_str("  captures\n");
            for (c, s) in p.captures.iter().enumerate() {
                let from = match *s {
                    CaptureSrc::Local(slot) => format!("local r{slot}"),
                    CaptureSrc::Capture(i) => format!("capture c{i}"),
                    CaptureSrc::SelfFn => "self".to_string(),
                };
                out.push_str(&format!("    c{c}  {from}\n"));
            }
        }
        out.push_str("  code\n");
        for (pc, ins) in p.code.iter().enumerate() {
            let (mnemonic, operands) = render(ins, interner);
            out.push_str(&format!(
                "    {pc:04}  {mnemonic:<11} {operands:<22} {}\n",
                position(p.lines[pc], src)
            ));
        }
    }

    /// The position half of ADR-023 point 2. `../reg-lisp`'s completeness test
    /// found *every* function's return attributed to line 0 — a hole a corpus
    /// does not surface, because the output still looks right — so the position
    /// prints on every instruction rather than where it seems interesting.
    fn position(o: SpanOrigin, src: &str) -> String {
        match o {
            SpanOrigin::Source(s) => {
                let (line, col) = line_col(src, s.start as usize);
                format!("{line}:{col}")
            }
            SpanOrigin::Generated(s) => {
                let (line, col) = line_col(src, s.start as usize);
                format!("generated {line}:{col}")
            }
            SpanOrigin::Unknown => "?".to_string(),
        }
    }

    fn render(i: &Instr, interner: &Interner) -> (&'static str, String) {
        match *i {
            Instr::Const { dst, k } => ("CONST", format!("r{dst} <- k{k}")),
            Instr::Move { dst, src } => ("MOVE", format!("r{dst} <- r{src}")),
            Instr::GetGlobal { dst, name } => {
                ("GETGLOBAL", format!("r{dst} <- {}", interner.name(name.0)))
            }
            Instr::SetGlobal { name, src } => {
                ("SETGLOBAL", format!("{} <- r{src}", interner.name(name.0)))
            }
            Instr::GetCapture { dst, idx } => ("GETCAP", format!("r{dst} <- c{idx}")),
            Instr::GetSelf { dst } => ("GETSELF", format!("r{dst} <- self")),
            Instr::SetCell { cell, src } => ("SETCELL", format!("[r{cell}] <- r{src}")),
            Instr::Closure { dst, proto } => ("CLOSURE", format!("r{dst} <- proto {proto}")),
            Instr::Call { dst, base, argc } => ("CALL", format!("r{dst} <- r{base}({argc})")),
            Instr::TailCall { base, argc } => ("TAILCALL", format!("r{base}({argc})")),
            Instr::Return { src } => ("RETURN", format!("r{src}")),
            Instr::Jump { target } => ("JUMP", format!("{target:04}")),
            Instr::JumpUnless { cond, target } => ("JUMPUNLESS", format!("r{cond}, {target:04}")),
            Instr::Throw { src } => ("THROW", format!("r{src}")),
            Instr::PushHandler { catch, err } => {
                ("PUSHHANDLER", format!("catch {catch:04}, err r{err}"))
            }
            Instr::PushFinally { target } => ("PUSHFINALLY", format!("{target:04}")),
            Instr::PopHandler => ("POPHANDLER", String::new()),
            Instr::EndFinally => ("ENDFINALLY", String::new()),
        }
    }
}

// ---------------------------------------------------------------------------

/// The core AST and the slot compiler (ADR-006, ADR-007, ADR-034).
///
/// Two passes. The resolver turns forms into `Core`, deciding what every symbol
/// *is* — a local, a capture, the running function itself, or a global — and
/// nothing about layout. The lowering assigns slots monotonically and emits
/// instructions. Splitting them is what keeps slot allocation in one place:
/// bindings and temporaries come out of the same counter, which is the whole of
/// "no liveness analysis, no reuse".
pub mod compile {
    use crate::bytecode::{
        CaptureIdx, CaptureSrc, Chunk, ConstIdx, Instr, Pc, Proto, ProtoIdx, Slot,
    };
    use crate::error::{LispErr, SpanOrigin};
    use crate::value::{kind_name, same_const, Interner, LocatedForm, Origins, SymId, Value};

    pub type LocalId = u32;

    // --- the core AST -------------------------------------------------------

    /// The closed core (ADR-007, amended by ADR-027, spelled by ADR-034). Read
    /// this enum and you have read the language.
    ///
    /// Thirteen forms, more variants: `literal` absorbs `quote`, and `local`
    /// arrives as one of three resolutions — a slot in this frame, a capture, or
    /// the running closure itself. Which one it is is the resolver's whole job.
    #[derive(Debug)]
    pub enum Core {
        Literal(Value),
        Local(LocalId),
        Capture(CaptureIdx),
        SelfFn,
        Global(SymId),
        If(Box<Expr>, Box<Expr>, Box<Expr>),
        Do(Vec<Expr>),
        Let(Vec<(LocalId, Expr)>, Vec<Expr>),
        Fn(Box<FnDef>),
        Call(Box<Expr>, Vec<Expr>),
        SetCell(Box<Expr>, Box<Expr>),
        SetGlobal(SymId, Box<Expr>),
        Throw(Box<Expr>),
        Try(Box<TryForm>),
        /// A re-entry into the enclosing `loop` (ADR-047). Lowered as a tail
        /// call to the loop's own function, which is what the loop is — so this
        /// is not a new kind of control flow, only one the compiler refuses to
        /// emit anywhere the existing tail-call rules would not.
        Recur(Vec<Expr>),
    }

    /// A core node and where it came from. The origin travels on every node
    /// because `lines[i]` needs one per *instruction* (ADR-023 point 2), and an
    /// instruction is emitted from a node.
    #[derive(Debug)]
    pub struct Expr {
        pub core: Core,
        pub origin: SpanOrigin,
    }

    #[derive(Debug)]
    pub struct FnDef {
        pub name: Option<SymId>,
        /// Named parameters, including the rest parameter when `variadic`.
        pub params: u32,
        pub variadic: bool,
        /// How many `LocalId`s this function hands out. Parameters are 0..params.
        pub locals: u32,
        pub captures: Vec<CaptureSpec>,
        pub body: Vec<Expr>,
        pub origin: SpanOrigin,
    }

    /// A capture named in the terms the *resolver* has — local ids, not slots.
    /// Slots do not exist until lowering, and the translation happens in the
    /// enclosing function, which is where the value is read from.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub enum CaptureSpec {
        Local(LocalId),
        Capture(CaptureIdx),
        SelfFn,
    }

    #[derive(Debug)]
    pub struct TryForm {
        pub body: Vec<Expr>,
        pub catch: Option<(LocalId, Vec<Expr>)>,
        pub finally: Option<Vec<Expr>>,
    }

    // --- walking forms ------------------------------------------------------

    /// A value and the origins travelling beside it (ADR-026). This pair is the
    /// unit the resolver walks; splitting them is exactly how spans get lost.
    #[derive(Clone, Copy)]
    struct Form<'a> {
        v: &'a Value,
        o: &'a Origins,
    }

    impl<'a> Form<'a> {
        /// Syntactic children, in source order, paired with their origins. A map
        /// contributes key then value, which is the order ADR-033 fixes for
        /// evaluation too.
        fn items(&self) -> Vec<Form<'a>> {
            let zip = |vs: Vec<&'a Value>| -> Vec<Form<'a>> {
                vs.into_iter()
                    .zip(&self.o.children)
                    .map(|(v, o)| Form { v, o })
                    .collect()
            };
            match self.v {
                Value::List(l) => zip(l.0.iter().collect()),
                Value::Vec(x) => zip(x.0.iter().collect()),
                Value::Map(m) => zip(m.0.iter().flat_map(|(k, v)| [k, v]).collect()),
                _ => Vec::new(),
            }
        }

        fn sym(&self) -> Option<SymId> {
            match self.v {
                Value::Sym(s) => Some(*s),
                _ => None,
            }
        }

        /// The head symbol of a non-empty list, which is what makes a form a
        /// special form or a call.
        fn head(&self) -> Option<SymId> {
            match self.v {
                Value::List(l) => match l.0.first() {
                    Some(Value::Sym(s)) => Some(*s),
                    _ => None,
                },
                _ => None,
            }
        }

        fn err(&self, msg: impl Into<String>) -> LispErr {
            LispErr::at_origin(self.o.origin, msg)
        }
    }

    // --- the resolver -------------------------------------------------------

    /// The core-form names, interned once. A head symbol is compared by id
    /// after this, which is the only comparison symbols are supposed to need.
    struct Specials {
        if_: SymId,
        do_: SymId,
        let_: SymId,
        fn_: SymId,
        quote: SymId,
        set_cell: SymId,
        set_global: SymId,
        throw: SymId,
        try_: SymId,
        catch: SymId,
        finally: SymId,
        rest: SymId,
        vector: SymId,
        hash_map: SymId,
        loop_: SymId,
        recur: SymId,
    }

    impl Specials {
        fn new(i: &mut Interner) -> Specials {
            let mut s = |n: &str| SymId(i.intern(n));
            Specials {
                if_: s("if"),
                do_: s("do"),
                let_: s("let"),
                fn_: s("fn"),
                quote: s("quote"),
                set_cell: s("set-cell!"),
                set_global: s("set-global!"),
                throw: s("throw"),
                try_: s("try"),
                catch: s("catch"),
                finally: s("finally"),
                rest: s("&"),
                vector: s("vector"),
                hash_map: s("hash-map"),
                loop_: s("loop"),
                recur: s("recur"),
            }
        }
    }

    struct FnScope {
        name: Option<SymId>,
        locals: u32,
        scopes: Vec<Vec<(SymId, LocalId)>>,
        captures: Vec<CaptureSpec>,
        /// `Some(n)` when this function *is* a `loop` of arity `n` (ADR-047).
        /// Only the innermost scope is consulted, which is what stops a `recur`
        /// inside a nested `fn` from targeting the loop outside it — that
        /// function's frame is not the loop's frame.
        loop_arity: Option<u32>,
    }

    impl FnScope {
        fn new(name: Option<SymId>) -> FnScope {
            FnScope {
                name,
                locals: 0,
                scopes: vec![Vec::new()],
                captures: Vec::new(),
                loop_arity: None,
            }
        }
    }

    struct Resolver<'a> {
        interner: &'a mut Interner,
        sp: Specials,
        fns: Vec<FnScope>,
    }

    /// Forms to core AST. Every symbol comes out resolved and every core form
    /// comes out shape-checked; nothing about slots has happened yet.
    pub fn resolve(forms: &[LocatedForm], interner: &mut Interner) -> Result<FnDef, LispErr> {
        let sp = Specials::new(interner);
        let mut r = Resolver {
            interner,
            sp,
            fns: vec![FnScope::new(None)],
        };
        let mut body = Vec::new();
        for f in forms {
            body.push(r.expr(Form {
                v: &f.root,
                o: &f.origins,
            })?);
        }
        let scope = r.fns.pop().expect("top-level scope");
        Ok(FnDef {
            name: None,
            params: 0,
            variadic: false,
            locals: scope.locals,
            captures: scope.captures,
            body,
            origin: forms
                .first()
                .map(|f| f.origins.origin)
                .unwrap_or(SpanOrigin::Unknown),
        })
    }

    impl Resolver<'_> {
        fn expr(&mut self, f: Form) -> Result<Expr, LispErr> {
            let origin = f.o.origin;
            let core = match f.v {
                // `()` evaluates to itself, as in Clojure. Every other list is a
                // special form or a call.
                Value::List(l) if l.0.is_empty() => Core::Literal(f.v.clone()),
                Value::List(_) => self.list(f)?,
                // Collection literals lower to calls (ADR-035), which is what
                // keeps the core closed at 13 forms while ADR-033's
                // left-to-right element order still falls out of ordinary
                // argument evaluation. Resolved as globals directly rather than
                // through scope: `[x]` is a vector literal even where a local
                // named `vector` is in scope.
                Value::Vec(_) => self.literal_call(f, self.sp.vector, origin)?,
                Value::Map(_) => self.literal_call(f, self.sp.hash_map, origin)?,
                Value::Sym(s) => self.symbol(*s),
                // ADR-023's cost paragraph: the compiler's input is not closed,
                // because a macro can put anything into code position. This is
                // the decent error it asks for.
                Value::Fn(_) | Value::Cell(_) | Value::Handle(_) | Value::Buffer(_) => {
                    return Err(f.err(format!("cannot compile a {}", kind_name(f.v))))
                }
                _ => Core::Literal(f.v.clone()),
            };
            Ok(Expr { core, origin })
        }

        fn literal_call(
            &mut self,
            f: Form,
            ctor: SymId,
            origin: SpanOrigin,
        ) -> Result<Core, LispErr> {
            let callee = Expr {
                core: Core::Global(ctor),
                origin,
            };
            Ok(Core::Call(Box::new(callee), self.exprs(&f.items())?))
        }

        fn exprs(&mut self, items: &[Form]) -> Result<Vec<Expr>, LispErr> {
            items.iter().map(|f| self.expr(*f)).collect()
        }

        fn symbol(&mut self, s: SymId) -> Core {
            let top = self.fns.len() - 1;
            self.lookup(top, s).unwrap_or(Core::Global(s))
        }

        /// A symbol resolves against the innermost function first, then outward,
        /// adding a capture at every level it crosses. A name that reaches no
        /// binding is a global, and stays unresolved until run time — which is
        /// what makes forward references free (ADR-034).
        fn lookup(&mut self, level: usize, name: SymId) -> Option<Core> {
            for scope in self.fns[level].scopes.iter().rev() {
                if let Some((_, id)) = scope.iter().rev().find(|(n, _)| *n == name) {
                    return Some(Core::Local(*id));
                }
            }
            if self.fns[level].name == Some(name) {
                return Some(Core::SelfFn);
            }
            if level == 0 {
                return None;
            }
            let spec = match self.lookup(level - 1, name)? {
                Core::Local(id) => CaptureSpec::Local(id),
                Core::Capture(i) => CaptureSpec::Capture(i),
                Core::SelfFn => CaptureSpec::SelfFn,
                _ => return None,
            };
            Some(Core::Capture(self.add_capture(level, spec)))
        }

        fn add_capture(&mut self, level: usize, spec: CaptureSpec) -> CaptureIdx {
            let caps = &mut self.fns[level].captures;
            if let Some(i) = caps.iter().position(|c| *c == spec) {
                return i as CaptureIdx;
            }
            caps.push(spec);
            (caps.len() - 1) as CaptureIdx
        }

        fn declare(&mut self, name: SymId) -> LocalId {
            let f = self.fns.last_mut().expect("a function scope");
            let id = f.locals;
            f.locals += 1;
            f.scopes.last_mut().expect("a block scope").push((name, id));
            id
        }

        fn push_scope(&mut self) {
            self.fns
                .last_mut()
                .expect("a function scope")
                .scopes
                .push(Vec::new());
        }

        fn pop_scope(&mut self) {
            self.fns
                .last_mut()
                .expect("a function scope")
                .scopes
                .pop()
                .expect("a block scope");
        }

        /// A list with a symbol head is a special form if the head names one.
        /// Checked before scope, so a core form cannot be shadowed by a local —
        /// the core is closed (ADR-007) and a language whose `if` depends on
        /// what is in scope has no closed core.
        fn list(&mut self, f: Form) -> Result<Core, LispErr> {
            let items = f.items();
            if let Some(h) = f.head() {
                let sp = &self.sp;
                if h == sp.if_ {
                    return self.if_form(f, &items);
                } else if h == sp.do_ {
                    return Ok(Core::Do(self.exprs(&items[1..])?));
                } else if h == sp.let_ {
                    return self.let_form(f, &items);
                } else if h == sp.loop_ {
                    return self.loop_form(f, &items);
                } else if h == sp.recur {
                    return self.recur_form(f, &items);
                } else if h == sp.fn_ {
                    return self.fn_form(f, &items);
                } else if h == sp.quote {
                    if items.len() != 2 {
                        return Err(f.err("`quote` takes exactly one form"));
                    }
                    return Ok(Core::Literal(items[1].v.clone()));
                } else if h == sp.set_cell {
                    if items.len() != 3 {
                        return Err(f.err("`set-cell!` takes a cell and a value"));
                    }
                    let cell = self.expr(items[1])?;
                    let val = self.expr(items[2])?;
                    return Ok(Core::SetCell(Box::new(cell), Box::new(val)));
                } else if h == sp.set_global {
                    return self.set_global_form(f, &items);
                } else if h == sp.throw {
                    if items.len() != 2 {
                        return Err(f.err("`throw` takes exactly one value"));
                    }
                    return Ok(Core::Throw(Box::new(self.expr(items[1])?)));
                } else if h == sp.try_ {
                    return self.try_form(&items);
                } else if h == sp.catch || h == sp.finally {
                    let name = self.interner.name(h.0).to_string();
                    return Err(f.err(format!("`{name}` is only valid inside `try`")));
                }
            }
            let callee = self.expr(items[0])?;
            Ok(Core::Call(Box::new(callee), self.exprs(&items[1..])?))
        }

        fn if_form(&mut self, f: Form, items: &[Form]) -> Result<Core, LispErr> {
            if items.len() < 3 || items.len() > 4 {
                return Err(f.err("`if` takes a test, a then, and an optional else"));
            }
            let test = self.expr(items[1])?;
            let then = self.expr(items[2])?;
            let other = match items.get(3) {
                Some(e) => self.expr(*e)?,
                // A missing else is `nil`, and it takes the `if`'s own position:
                // there is no source text to point at, and `Unknown` here would
                // lose the one thing a backtrace could say.
                None => Expr {
                    core: Core::Literal(Value::Nil),
                    origin: f.o.origin,
                },
            };
            Ok(Core::If(Box::new(test), Box::new(then), Box::new(other)))
        }

        fn let_form(&mut self, f: Form, items: &[Form]) -> Result<Core, LispErr> {
            if items.len() < 2 {
                return Err(f.err("`let` takes a binding vector and a body"));
            }
            if !matches!(items[1].v, Value::Vec(_)) {
                return Err(items[1].err(format!(
                    "`let` takes a binding vector, not a {}",
                    kind_name(items[1].v)
                )));
            }
            let pairs = items[1].items();
            if !pairs.len().is_multiple_of(2) {
                return Err(items[1].err("binding vector has a name with no value"));
            }
            self.push_scope();
            let mut binds = Vec::new();
            for pair in pairs.chunks(2) {
                let name = pair[0].sym().ok_or_else(|| {
                    pair[0].err(format!(
                        "`let` binds symbols, not a {}",
                        kind_name(pair[0].v)
                    ))
                })?;
                // Sequential, left to right (ADR-033): the initializer is
                // resolved before its own name is in scope, so `(let [x x] ...)`
                // takes the outer `x`.
                let init = self.expr(pair[1])?;
                binds.push((self.declare(name), init));
            }
            let body = self.exprs(&items[2..])?;
            self.pop_scope();
            Ok(Core::Let(binds, body))
        }

        /// `(loop [a init-a b init-b] body…)`. ADR-047.
        ///
        /// A `let` for the bindings, whose body immediately calls an anonymous
        /// function taking those names. `recur` is then a tail call to that
        /// function, so **"tail position for `recur`" is the flag the compiler
        /// already threads for every other tail call** rather than a second
        /// opinion about what tail position means. That single definition is the
        /// whole reason this is a core form and not the eight-line prelude macro
        /// (`notes/loop-recur-attempt.md`).
        ///
        /// The outer `let` is not ceremony: it keeps Clojure's sequential
        /// bindings, so `(loop [a 1 b a] …)` sees `a`. Passing the initializers
        /// straight in as arguments would evaluate them all in the outer scope
        /// and change that silently.
        fn loop_form(&mut self, f: Form, items: &[Form]) -> Result<Core, LispErr> {
            if items.len() < 2 {
                return Err(f.err("`loop` takes a binding vector and a body"));
            }
            if !matches!(items[1].v, Value::Vec(_)) {
                return Err(items[1].err(format!(
                    "`loop` takes a binding vector, not a {}",
                    kind_name(items[1].v)
                )));
            }
            let pairs = items[1].items();
            if !pairs.len().is_multiple_of(2) {
                return Err(items[1].err("binding vector has a name with no value"));
            }
            let arity = (pairs.len() / 2) as u32;
            let o = f.o.origin;

            self.push_scope();
            let mut binds = Vec::new();
            let mut names = Vec::new();
            for pair in pairs.chunks(2) {
                let name = pair[0].sym().ok_or_else(|| {
                    pair[0].err(format!(
                        "`loop` binds symbols, not a {}",
                        kind_name(pair[0].v)
                    ))
                })?;
                let init = self.expr(pair[1])?;
                binds.push((self.declare(name), init));
                names.push(name);
            }

            // The function `recur` re-enters. Its parameters are the loop names
            // declared again, in its own scope, so the body sees the parameters
            // and not the outer bindings they were initialised from.
            let mut scope = FnScope::new(None);
            scope.loop_arity = Some(arity);
            self.fns.push(scope);
            for n in &names {
                self.declare(*n);
            }
            let built = self.exprs(&items[2..]);
            let scope = self.fns.pop().expect("the loop scope just pushed");
            let body = built?;
            self.pop_scope();

            let args = binds
                .iter()
                .map(|(id, _)| Expr {
                    core: Core::Local(*id),
                    origin: o,
                })
                .collect();
            let callee = Expr {
                core: Core::Fn(Box::new(FnDef {
                    name: None,
                    params: arity,
                    variadic: false,
                    locals: scope.locals,
                    captures: scope.captures,
                    body,
                    origin: o,
                })),
                origin: o,
            };
            Ok(Core::Let(
                binds,
                vec![Expr {
                    core: Core::Call(Box::new(callee), args),
                    origin: o,
                }],
            ))
        }

        /// `(recur e…)`. Valid only as the innermost function's own re-entry,
        /// which is exactly "inside a `loop`, not across a `fn`". Arity is
        /// checked here rather than at the call, because the loop's arity is
        /// known and a mismatch has a better message than the callee's.
        fn recur_form(&mut self, f: Form, items: &[Form]) -> Result<Core, LispErr> {
            let Some(arity) = self.fns.last().and_then(|s| s.loop_arity) else {
                return Err(
                    f.err("`recur` is only valid inside `loop`, and never across a `fn` boundary")
                );
            };
            let given = items.len() as u32 - 1;
            if given != arity {
                return Err(f.err(format!(
                    "`recur` rebinds the {arity} name(s) its `loop` binds, given {given}"
                )));
            }
            Ok(Core::Recur(self.exprs(&items[1..])?))
        }

        fn fn_form(&mut self, f: Form, items: &[Form]) -> Result<Core, LispErr> {
            let mut at = 1;
            let name = match items.get(at).and_then(|i| i.sym()) {
                Some(s) => {
                    at += 1;
                    Some(s)
                }
                None => None,
            };
            let params = match items.get(at) {
                Some(p) if matches!(p.v, Value::Vec(_)) => *p,
                Some(p) => {
                    return Err(p.err(format!(
                        "`fn` takes a parameter vector, not a {}",
                        kind_name(p.v)
                    )))
                }
                None => return Err(f.err("`fn` takes a parameter vector and a body")),
            };
            self.fns.push(FnScope::new(name));
            // Popped whether or not the body resolved, so an error does not
            // leave the resolver holding a scope that no longer exists.
            let built = self.fn_inner(params, &items[at + 1..]);
            let scope = self.fns.pop().expect("the function scope just pushed");
            let (count, variadic, body) = built?;
            Ok(Core::Fn(Box::new(FnDef {
                name,
                params: count,
                variadic,
                locals: scope.locals,
                captures: scope.captures,
                body,
                origin: f.o.origin,
            })))
        }

        fn fn_inner(
            &mut self,
            params: Form,
            body: &[Form],
        ) -> Result<(u32, bool, Vec<Expr>), LispErr> {
            let (count, variadic) = self.params(params)?;
            Ok((count, variadic, self.exprs(body)?))
        }

        /// `[a b & rest]`. One parameter list, optionally ending in `&` and one
        /// rest parameter (ADR-033 rule 3). Parameters are declared first, so
        /// they hold local ids `0..params` and therefore slots `0..params` —
        /// which is where a call leaves the arguments (ADR-034).
        fn params(&mut self, f: Form) -> Result<(u32, bool), LispErr> {
            let items = f.items();
            let mut count = 0;
            let mut i = 0;
            while i < items.len() {
                let name = items[i].sym().ok_or_else(|| {
                    items[i].err(format!(
                        "parameters are symbols, not a {}",
                        kind_name(items[i].v)
                    ))
                })?;
                if name == self.sp.rest {
                    let rest = items.get(i + 1).and_then(|p| p.sym());
                    match rest {
                        Some(r) if r != self.sp.rest && i + 2 == items.len() => {
                            self.declare(r);
                            return Ok((count + 1, true));
                        }
                        _ => {
                            return Err(
                                items[i].err("`&` must be followed by exactly one rest parameter")
                            )
                        }
                    }
                }
                self.declare(name);
                count += 1;
                i += 1;
            }
            Ok((count, false))
        }

        fn set_global_form(&mut self, f: Form, items: &[Form]) -> Result<Core, LispErr> {
            if items.len() != 3 {
                return Err(f.err("`set-global!` takes a name and a value"));
            }
            let name = items[1].sym().ok_or_else(|| {
                items[1].err(format!(
                    "`set-global!` binds a symbol, not a {}",
                    kind_name(items[1].v)
                ))
            })?;
            Ok(Core::SetGlobal(name, Box::new(self.expr(items[2])?)))
        }

        fn try_form(&mut self, items: &[Form]) -> Result<Core, LispErr> {
            let mut body = Vec::new();
            let mut catch = None;
            let mut finally = None;
            for it in &items[1..] {
                let head = it.head();
                if head == Some(self.sp.catch) {
                    if catch.is_some() {
                        return Err(it.err("`try` takes at most one `catch` (Q23)"));
                    }
                    if finally.is_some() {
                        return Err(it.err("`finally` must be the last clause"));
                    }
                    let ci = it.items();
                    let bound = ci
                        .get(1)
                        .and_then(|c| c.sym())
                        .ok_or_else(|| it.err("`catch` binds the thrown value to one symbol"))?;
                    self.push_scope();
                    let id = self.declare(bound);
                    let handler = self.exprs(&ci[2..]);
                    self.pop_scope();
                    catch = Some((id, handler?));
                } else if head == Some(self.sp.finally) {
                    if finally.is_some() {
                        return Err(it.err("`try` takes at most one `finally`"));
                    }
                    finally = Some(self.exprs(&it.items()[1..])?);
                } else if catch.is_some() || finally.is_some() {
                    return Err(
                        it.err("everything after a `catch` or `finally` clause must be one too")
                    );
                } else {
                    body.push(self.expr(*it)?);
                }
            }
            Ok(Core::Try(Box::new(TryForm {
                body,
                catch,
                finally,
            })))
        }
    }

    // --- lowering -----------------------------------------------------------

    pub fn compile(forms: &[LocatedForm], interner: &mut Interner) -> Result<Chunk, LispErr> {
        let mut chunk = Chunk { protos: Vec::new() };
        compile_into(&mut chunk, forms, interner)?;
        Ok(chunk)
    }

    /// Compile into an existing chunk, appending. Returns the index of the
    /// top-level proto this call added.
    ///
    /// ADR-044 part 2: a REPL session has one chunk, and each input extends it.
    /// Existing indices never move, which is the whole trick — a closure from
    /// an earlier input keeps naming a valid proto, so Q29's registry is not
    /// needed to make one input's function callable from the next.
    pub fn compile_into(
        chunk: &mut Chunk,
        forms: &[LocatedForm],
        interner: &mut Interner,
    ) -> Result<u32, LispErr> {
        let top = resolve(forms, interner)?;
        // Seeded with what is already there, so the counter that hands out
        // proto indices continues rather than restarting. Every index a new
        // instruction names is therefore an index into the whole chunk.
        let mut lo = Lower {
            protos: std::mem::take(&mut chunk.protos)
                .into_iter()
                .map(Some)
                .collect(),
            err: None,
        };
        let existing = lo.protos.len();
        let idx = lo.proto(&top, Vec::new());
        if let Some(e) = lo.err {
            // Put the chunk back exactly as it was. A refused compile must not
            // leave its protos behind in a REPL session's chunk (ADR-044) — the
            // next input would be numbered past code that never ran, and every
            // index after it would name the wrong function.
            chunk.protos = lo
                .protos
                .into_iter()
                .take(existing)
                .map(|p| p.expect("an already-compiled proto is always filled in"))
                .collect();
            return Err(e);
        }
        chunk.protos = lo
            .protos
            .into_iter()
            .map(|p| p.expect("every reserved proto is filled in"))
            .collect();
        Ok(idx as u32)
    }

    /// Core AST to a `Chunk`. Slots come out of one monotonic counter per
    /// function, shared by bindings and temporaries and never reused (ADR-006).
    /// E-5 is what makes that affordable here: with unpacked operands a high
    /// slot index costs nothing at all.
    pub fn lower(top: &FnDef) -> Result<Chunk, LispErr> {
        let mut lo = Lower {
            protos: Vec::new(),
            err: None,
        };
        lo.proto(top, Vec::new());
        // Fallible since ADR-047: lowering is where tail position is known, so
        // it is where a misplaced `recur` is caught.
        if let Some(e) = lo.err {
            return Err(e);
        }
        Ok(Chunk {
            protos: lo
                .protos
                .into_iter()
                .map(|p| p.expect("every reserved proto is filled in"))
                .collect(),
        })
    }

    struct Lower {
        /// An index is reserved before its body is lowered, so a proto's number
        /// is its source order rather than the order compilation finished.
        protos: Vec<Option<Proto>>,
        /// The first refusal, kept until the whole tree has been walked.
        ///
        /// Lowering is otherwise infallible, and ADR-047 needed one thing from
        /// it that resolution cannot answer: whether a `recur` is in tail
        /// position. Stashing the error here rather than making every lowering
        /// method return `Result` is what keeps that judgement in the one place
        /// that already makes it.
        err: Option<LispErr>,
    }

    impl Lower {
        /// First one wins. A `recur` in a bad position often makes the ones
        /// after it look wrong too, and the first is the one to fix.
        fn fail(&mut self, e: LispErr) {
            if self.err.is_none() {
                self.err = Some(e);
            }
        }
    }

    impl Lower {
        fn proto(&mut self, def: &FnDef, captures: Vec<CaptureSrc>) -> ProtoIdx {
            let idx = self.protos.len();
            self.protos.push(None);
            let mut f = FnLower::new(def);
            let dst = f.alloc(1);
            f.body(self, &def.body, dst, true, def.origin);
            // The return belongs to the expression whose value it returns, not
            // to the `fn` form. For the top-level proto those differ by the
            // whole file: `def.origin` is the *first* form, so a `RETURN` was
            // reporting a position with no relationship to it (ADR-023 point 2
            // exists so a frame's position means something).
            //
            // Unreachable after a tail call, and emitted anyway. One dead
            // instruction is cheaper than a special case in the single place a
            // function's exit is guaranteed to exist.
            let exit = def.body.last().map(|e| e.origin).unwrap_or(def.origin);
            f.emit(Instr::Return { src: dst }, exit);
            self.protos[idx] = Some(Proto {
                name: def.name,
                params: def.params,
                variadic: def.variadic,
                slots: f.next,
                code: f.code,
                lines: f.lines,
                consts: f.consts,
                captures,
            });
            idx as ProtoIdx
        }
    }

    struct FnLower {
        code: Vec<Instr>,
        lines: Vec<SpanOrigin>,
        consts: Vec<Value>,
        /// `LocalId` to slot, filled in at the binding, read at every use.
        slots: Vec<Slot>,
        next: Slot,
        /// Handler regions open in this function. A call inside one is never a
        /// tail call — ADR-028 rule 2 says the frame is still needed, and that
        /// reason covers `catch` exactly as it covers `finally`: the handler
        /// record names this frame, and a reused frame is a different one.
        regions: u32,
    }

    impl FnLower {
        fn new(def: &FnDef) -> FnLower {
            let mut slots = vec![0; def.locals as usize];
            // Parameters were declared first, so they hold local ids
            // `0..params` — and a call leaves the arguments in exactly those
            // slots (ADR-034).
            for i in 0..def.params {
                slots[i as usize] = i;
            }
            FnLower {
                code: Vec::new(),
                lines: Vec::new(),
                consts: Vec::new(),
                slots,
                next: def.params,
                regions: 0,
            }
        }

        /// The only place an instruction is created, which is what keeps `lines`
        /// parallel to `code` structurally rather than by discipline (ADR-023
        /// point 2). `../reg-lisp` lost that parallel in a mutant its whole
        /// suite failed to notice.
        fn emit(&mut self, i: Instr, o: SpanOrigin) -> Pc {
            self.code.push(i);
            self.lines.push(o);
            (self.code.len() - 1) as Pc
        }

        fn here(&self) -> Pc {
            self.code.len() as Pc
        }

        fn alloc(&mut self, n: u32) -> Slot {
            let s = self.next;
            self.next += n;
            s
        }

        fn patch(&mut self, at: Pc, target: Pc) {
            match &mut self.code[at as usize] {
                Instr::Jump { target: t }
                | Instr::JumpUnless { target: t, .. }
                | Instr::PushFinally { target: t }
                | Instr::PushHandler { catch: t, .. } => *t = target,
                other => unreachable!("cannot patch {other:?}"),
            }
        }

        /// Constants are deduplicated by `same_const`, which is neither Rust's
        /// `PartialEq` nor the language's `=`. Two constants share an entry
        /// only when no program can tell them apart, so `1` never merges with
        /// `1.0`, a list never merges with a vector, and — the one that
        /// actually bit — `0.0` never merges with `-0.0`.
        fn konst(&mut self, v: &Value, dst: Slot, o: SpanOrigin) {
            let k = match self.consts.iter().position(|c| same_const(c, v)) {
                Some(k) => k,
                None => {
                    self.consts.push(v.clone());
                    self.consts.len() - 1
                }
            };
            self.emit(
                Instr::Const {
                    dst,
                    k: k as ConstIdx,
                },
                o,
            );
        }

        /// An implicit `do`: every form but the last runs for effect, the last
        /// supplies the value. An empty body is `nil`.
        fn body(&mut self, lo: &mut Lower, exprs: &[Expr], dst: Slot, tail: bool, o: SpanOrigin) {
            match exprs.split_last() {
                None => self.konst(&Value::Nil, dst, o),
                Some((last, rest)) => {
                    for e in rest {
                        let scratch = self.alloc(1);
                        self.expr(lo, e, scratch, false);
                    }
                    self.expr(lo, last, dst, tail);
                }
            }
        }

        fn expr(&mut self, lo: &mut Lower, e: &Expr, dst: Slot, tail: bool) {
            let o = e.origin;
            match &e.core {
                Core::Literal(v) => self.konst(v, dst, o),
                Core::Local(id) => {
                    // No `src == dst` guard: `dst` is always an `alloc` result,
                    // `alloc` never returns a slot twice, and a local's slot is
                    // either a parameter index or a different `alloc` result —
                    // so the two can never coincide. A guard here would be dead
                    // code that reads as a peephole optimization, which is the
                    // shape the milestone-2 mutation pass already found once.
                    let src = self.slots[*id as usize];
                    self.emit(Instr::Move { dst, src }, o);
                }
                Core::Capture(i) => {
                    self.emit(Instr::GetCapture { dst, idx: *i }, o);
                }
                Core::SelfFn => {
                    self.emit(Instr::GetSelf { dst }, o);
                }
                Core::Global(s) => {
                    self.emit(Instr::GetGlobal { dst, name: *s }, o);
                }
                Core::If(test, then, other) => {
                    let cond = self.alloc(1);
                    self.expr(lo, test, cond, false);
                    let jump_else = self.emit(Instr::JumpUnless { cond, target: 0 }, o);
                    self.expr(lo, then, dst, tail);
                    let jump_end = self.emit(Instr::Jump { target: 0 }, o);
                    let els = self.here();
                    self.patch(jump_else, els);
                    self.expr(lo, other, dst, tail);
                    let end = self.here();
                    self.patch(jump_end, end);
                }
                Core::Do(es) => self.body(lo, es, dst, tail, o),
                Core::Let(binds, es) => {
                    for (id, init) in binds {
                        let s = self.alloc(1);
                        self.expr(lo, init, s, false);
                        self.slots[*id as usize] = s;
                    }
                    self.body(lo, es, dst, tail, o);
                }
                Core::Fn(def) => {
                    // Capture sources are named in the enclosing frame, so they
                    // are translated to slots here rather than inside the nested
                    // function (ADR-002: copied at creation, never referenced).
                    let captures = def
                        .captures
                        .iter()
                        .map(|c| match *c {
                            CaptureSpec::Local(id) => CaptureSrc::Local(self.slots[id as usize]),
                            CaptureSpec::Capture(i) => CaptureSrc::Capture(i),
                            CaptureSpec::SelfFn => CaptureSrc::SelfFn,
                        })
                        .collect();
                    let proto = lo.proto(def, captures);
                    self.emit(Instr::Closure { dst, proto }, o);
                }
                Core::Call(callee, args) => {
                    let argc = args.len() as u32;
                    // The whole window is reserved before anything is lowered
                    // into it, so a nested call inside an argument allocates
                    // above it rather than through it.
                    let base = self.alloc(1 + argc);
                    self.expr(lo, callee, base, false);
                    for (i, a) in args.iter().enumerate() {
                        self.expr(lo, a, base + 1 + i as u32, false);
                    }
                    if tail && self.regions == 0 {
                        self.emit(Instr::TailCall { base, argc }, o);
                    } else {
                        self.emit(Instr::Call { dst, base, argc }, o);
                    }
                }
                Core::SetCell(cell, val) => {
                    let c = self.alloc(1);
                    self.expr(lo, cell, c, false);
                    // Left to right (ADR-033), and the form's value is the value
                    // written — so it is lowered straight into `dst`.
                    self.expr(lo, val, dst, false);
                    self.emit(Instr::SetCell { cell: c, src: dst }, o);
                }
                Core::SetGlobal(name, val) => {
                    self.expr(lo, val, dst, false);
                    self.emit(
                        Instr::SetGlobal {
                            name: *name,
                            src: dst,
                        },
                        o,
                    );
                }
                Core::Throw(v) => {
                    let src = self.alloc(1);
                    self.expr(lo, v, src, false);
                    self.emit(Instr::Throw { src }, o);
                }
                Core::Try(t) => self.try_form(lo, t, dst, tail, o),
                Core::Recur(args) => {
                    // The loop *is* a function (ADR-047), so re-entering it is a
                    // tail call to itself and the conditions are the ones every
                    // tail call already has to meet. Nothing here decides what
                    // tail position is; it reads the decision.
                    if !tail {
                        lo.fail(LispErr::at_origin(
                            o,
                            "`recur` must be in tail position, because it re-enters the \
                             loop rather than returning a value to its caller",
                        ));
                    } else if self.regions != 0 {
                        // ADR-028 rule 2. Jumping back into the loop from inside
                        // a handler region would leave the handler record on the
                        // stack and skip the cleanup it names.
                        lo.fail(LispErr::at_origin(
                            o,
                            "`recur` cannot cross a `try`: re-entering the loop would \
                             skip the cleanup and leave the handler installed",
                        ));
                    }
                    let argc = args.len() as u32;
                    let base = self.alloc(1 + argc);
                    self.emit(Instr::GetSelf { dst: base }, o);
                    for (i, a) in args.iter().enumerate() {
                        self.expr(lo, a, base + 1 + i as u32, false);
                    }
                    self.emit(Instr::TailCall { base, argc }, o);
                }
            }
        }

        /// Two nested handler regions, with `finally` emitted twice — once on
        /// the normal path, once as the path the VM enters while unwinding
        /// (ADR-034). Nesting the catch region *inside* the finally region is
        /// what makes a throw from the catch body still run the cleanup.
        ///
        /// The protocol milestone 4 has to honour: the VM pops a handler record
        /// when it dispatches to that record's target, and this code pops it
        /// with `POPHANDLER` on the path where nothing was thrown. Exactly one
        /// of those happens per record, which is ADR-028 invariant 1 read off
        /// the emitted code rather than argued about a state machine.
        fn try_form(&mut self, lo: &mut Lower, t: &TryForm, dst: Slot, tail: bool, o: SpanOrigin) {
            let fin = t.finally.as_ref().map(|_| {
                self.regions += 1;
                self.emit(Instr::PushFinally { target: 0 }, o)
            });
            let cat = t.catch.as_ref().map(|(id, _)| {
                let err = self.alloc(1);
                self.slots[*id as usize] = err;
                self.regions += 1;
                self.emit(Instr::PushHandler { catch: 0, err }, o)
            });

            // `tail` passes through untouched. Whether a call in the protected
            // body is *actually* a tail call is the `regions` counter's
            // business, decided where the call is emitted — one mechanism
            // rather than two that have to agree. Clearing the flag here as
            // well read as belt-and-braces and was really a way for the counter
            // to be dead and untestable: a mutation that deleted it left the
            // whole suite green (`docs/notes/milestone-2-mutants.md`).
            self.body(lo, &t.body, dst, tail, o);

            if let (Some(push), Some((_, handler))) = (cat, t.catch.as_ref()) {
                self.emit(Instr::PopHandler, o);
                self.regions -= 1;
                let over = self.emit(Instr::Jump { target: 0 }, o);
                let entry = self.here();
                self.patch(push, entry);
                self.body(lo, handler, dst, tail, o);
                let done = self.here();
                self.patch(over, done);
            }

            if let (Some(push), Some(cleanup)) = (fin, t.finally.as_ref()) {
                self.emit(Instr::PopHandler, o);
                self.regions -= 1;
                // The cleanup's value is discarded on both paths, so it is never
                // in tail position and wants a slot only to be written to.
                let normal = self.alloc(1);
                self.body(lo, cleanup, normal, false, o);
                let over = self.emit(Instr::Jump { target: 0 }, o);
                let entry = self.here();
                self.patch(push, entry);
                let unwinding = self.alloc(1);
                self.body(lo, cleanup, unwinding, false, o);
                self.emit(Instr::EndFinally, o);
                let end = self.here();
                self.patch(over, end);
            }
        }
    }
}

// ---------------------------------------------------------------------------

/// The VM: frames, calls, closures, tail calls, and the handler stack
/// (ADR-004, ADR-038, ADR-039).
///
/// The dispatch loop is flat and the Rust stack is empty at every instruction
/// boundary (ADR-004). That is the decision the whole snapshot story rests on,
/// and it is also why nothing here recurses — a native function is called and
/// returns, but it never re-enters the loop.
///
/// `Vm` and `Execution` are separate for the reason ADR-029 gives: an `Image` is
/// one of each, so anything not in either is out of scope by construction.
/// Milestones 3 and 4 build both; fuel is milestone 8.
pub mod vm {
    use crate::bytecode::{CaptureSrc, Chunk, Instr, Slot};
    use crate::error::SpanOrigin;
    pub use crate::host::{IoKind, IoOp};
    use crate::value::{
        CellId, Closure, HandleId, Interner, KwId, ListObj, MapObj, NativeId, StrObj, SymId, Value,
    };
    use std::rc::Rc;

    /// How a run ended. Both endings are language values (ADR-039): a fault the
    /// VM raises unwinds exactly as `throw` does, so there is one failure path
    /// and `Threw` is all of it.
    #[derive(Debug)]
    pub enum Outcome {
        Returned(Value),
        Threw(Unwind),
        /// Fuel ran out at an instruction boundary (ADR-029, ADR-043 part 4).
        /// It carries nothing: the state is the `Execution`, which the caller
        /// already holds, and duplicating any of it here would be a second
        /// place for it to be wrong.
        Suspended,
    }

    /// A failure in flight. The value is what a `catch` binds; the origin and
    /// the suppressed chain travel *beside* it rather than inside it (ADR-039
    /// clause 4). That is ADR-026's rule for origins, and it is the only shape
    /// that still works when the thrown value is an integer.
    #[derive(Debug)]
    pub struct Unwind {
        pub value: Value,
        /// The instruction that raised it — the only place the position is
        /// known, because a value carries none.
        pub origin: SpanOrigin,
        /// Errors this one displaced, newest first (ADR-028 invariant 3).
        pub suppressed: Vec<Value>,
    }

    impl Unwind {
        pub fn new(value: Value, origin: SpanOrigin) -> Unwind {
            Unwind {
                value,
                origin,
                suppressed: Vec::new(),
            }
        }

        /// `path:line:col` for the instruction that raised it, where known.
        pub fn position(&self, path: &str, src: &str) -> Option<String> {
            let s = self.origin.span()?;
            let (line, col) = crate::error::line_col(src, s.start as usize);
            Some(format!("{path}:{line}:{col}"))
        }

        /// Take over an error this one displaced, chain and all.
        fn suppress(&mut self, other: Unwind) {
            self.suppressed.push(other.value);
            self.suppressed.extend(other.suppressed);
        }
    }

    /// The closed `:kind` vocabulary for `:type :vm-error` (ADR-039 clause 3).
    /// Adding one is an ADR, not a new keyword at a raise site — an open set of
    /// keywords is a formatted string wearing a colon.
    #[derive(Clone, Copy, Debug)]
    pub enum Kind {
        Arity,
        Unbound,
        NotCallable,
        Type,
        Overflow,
        /// Integer division by zero (ADR-041). Floats reach `##Inf` instead,
        /// which is IEEE's answer and not an error.
        DivideByZero,
        /// A decision this language has deliberately not taken, reached at run
        /// time. Q26's floats are the only one so far.
        Undecided,
        /// A VM invariant no program should be able to reach.
        Internal,
    }

    impl Kind {
        /// In discriminant order, which is what `kind as usize` indexes.
        const ALL: [Kind; 8] = [
            Kind::Arity,
            Kind::Unbound,
            Kind::NotCallable,
            Kind::Type,
            Kind::Overflow,
            Kind::DivideByZero,
            Kind::Undecided,
            Kind::Internal,
        ];

        fn name(self) -> &'static str {
            match self {
                Kind::Arity => "arity",
                Kind::Unbound => "unbound",
                Kind::NotCallable => "not-callable",
                Kind::Type => "type",
                Kind::Overflow => "overflow",
                Kind::DivideByZero => "divide-by-zero",
                Kind::Undecided => "undecided",
                Kind::Internal => "internal",
            }
        }
    }

    /// A fault before it has a position: what went wrong, and how to say it to
    /// a human. Natives raise these; the dispatch loop turns one into an
    /// `Unwind` at the instruction that raised it.
    ///
    /// Two variants because ADR-039 closes `:kind` *within* a `:type`, and
    /// ADR-042 adds the second `:type`. An io failure carries the operation and
    /// (where the operation names one) the path beside its kind; a VM fault has
    /// neither.
    #[derive(Debug)]
    pub enum Fault {
        Vm {
            kind: Kind,
            msg: String,
        },
        Io {
            op: IoOp,
            path: Option<String>,
            kind: IoKind,
            msg: String,
        },
    }

    pub fn fault(kind: Kind, msg: impl Into<String>) -> Fault {
        Fault::Vm {
            kind,
            msg: msg.into(),
        }
    }

    pub type NativeFn = fn(&mut Vm, &[Value]) -> Result<Value, Fault>;

    struct Native {
        name: SymId,
        /// Minimum argument count; `variadic` allows more.
        min: u32,
        variadic: bool,
        f: NativeFn,
    }

    /// ADR-025: cells are retained for the lifetime of the VM in v1, and the
    /// live count is instrumented rather than reclaimed.
    pub(crate) struct CellEntry {
        pub(crate) generation: u32,
        pub(crate) value: Value,
    }

    /// ADR-016: handles are generational, so a stale handle is an error rather
    /// than a silent alias.
    ///
    /// This is the cell arena's shape with the half ADR-025 does not need: a
    /// cell lives as long as the VM, so its generation is written once and
    /// never bumped, while a handle is the first thing here that frees a slot
    /// and hands the index back out. The generation is what keeps the reuse
    /// from being an alias (ADR-042 part 4).
    pub(crate) struct HandleEntry {
        pub(crate) generation: u32,
        /// `None` once closed. The slot outlives the resource so a stale id can
        /// still be recognised as stale rather than falling off the end.
        pub(crate) host: Option<crate::host::Host>,
    }

    /// The keywords a fault value is built from, interned once at construction
    /// (ADR-039 clause 3). The closed vocabulary is readable here, in one
    /// place, instead of being spread over the raise sites.
    struct Kws {
        type_: KwId,
        kind: KwId,
        message: KwId,
        vm_error: KwId,
        /// Indexed by `Kind as usize`.
        kinds: Vec<KwId>,
        /// ADR-042: the second `:type`, and the two keys only it carries.
        io_error: KwId,
        operation: KwId,
        path: KwId,
        /// Indexed by `IoKind as usize` and `IoOp as usize`. The operations are
        /// a closed list for the same reason the kinds are: `:operation` is a
        /// keyword, and interning one at a raise site would need `&mut Vm`
        /// where building a fault value has only `&Vm`.
        io_kinds: Vec<KwId>,
        io_ops: Vec<KwId>,
    }

    pub struct Vm {
        pub interner: Interner,
        /// Indexed by `SymId`, not a map: globals reach output through the
        /// disassembler and eventually through an `Image`, and a `Vec` is
        /// deterministic by construction where a `HashMap` needs a sort
        /// (BUILD.md, determinism).
        pub(crate) globals: Vec<Option<CellId>>,
        pub(crate) cells: Vec<CellEntry>,
        /// ADR-016: the VM owns the handle table, not the host module. What
        /// `host` owns is what a handle *is*.
        pub(crate) handles: Vec<HandleEntry>,
        /// Indices whose resource has been closed, available for reuse under a
        /// bumped generation. A `Vec` and not a free-list threaded through the
        /// entries, because the entries have to stay readable.
        pub(crate) free_handles: Vec<u32>,
        natives: Vec<Native>,
        kws: Kws,
        /// ADR-040: reset per compilation unit by the expander, never here.
        /// A counter that survived a unit would make the same source expand
        /// differently on its second run, and a golden cannot pin that.
        pub(crate) gensym: u64,
        /// The buffered in-memory host BUILD.md's serialization property needs:
        /// emitted effects are part of the comparison rather than escaping it.
        pub(crate) out: String,
    }

    impl Vm {
        pub fn new() -> Vm {
            let mut interner = Interner::new();
            let kws = Kws {
                type_: KwId(interner.intern("type")),
                kind: KwId(interner.intern("kind")),
                message: KwId(interner.intern("message")),
                vm_error: KwId(interner.intern("vm-error")),
                kinds: Kind::ALL
                    .iter()
                    .map(|k| KwId(interner.intern(k.name())))
                    .collect(),
                io_error: KwId(interner.intern("io-error")),
                operation: KwId(interner.intern("operation")),
                path: KwId(interner.intern("path")),
                io_kinds: IoKind::ALL
                    .iter()
                    .map(|k| KwId(interner.intern(k.name())))
                    .collect(),
                io_ops: IoOp::ALL
                    .iter()
                    .map(|o| KwId(interner.intern(o.name())))
                    .collect(),
            };
            debug_assert!(
                Kind::ALL.iter().enumerate().all(|(i, k)| *k as usize == i),
                "Kind::ALL is not in discriminant order, so `kind as usize` indexes the wrong keyword"
            );
            debug_assert!(
                IoKind::ALL.iter().enumerate().all(|(i, k)| *k as usize == i)
                    && IoOp::ALL.iter().enumerate().all(|(i, o)| *o as usize == i),
                "an io vocabulary is not in discriminant order, so `as usize` indexes the wrong keyword"
            );
            let mut vm = Vm {
                interner,
                globals: Vec::new(),
                cells: Vec::new(),
                handles: Vec::new(),
                free_handles: Vec::new(),
                natives: Vec::new(),
                kws,
                gensym: 0,
                out: String::new(),
            };
            // The one edge of the `prim` seam: cut that module out and this
            // line is the only thing that stops compiling, which is what
            // ETHOS.md means by a boundary that exists for subtraction. `host`
            // is the same shape — ADR-013's gating happens here, at install,
            // and never in the dispatch loop.
            crate::prim::install(&mut vm);
            crate::host::install(&mut vm);
            crate::adapters::install(&mut vm);
            vm
        }

        /// ADR-039 clause 3 and ADR-042 part 1: a raised fault is a language
        /// value of exactly one of these two shapes. `:kind` is the contract;
        /// `:message` is prose.
        fn fault_value(&self, f: &Fault) -> Value {
            let kw = Value::Keyword;
            let text = |s: &String| Value::Str(Rc::new(StrObj(s.clone())));
            let pairs = match f {
                Fault::Vm { kind, msg } => vec![
                    (kw(self.kws.type_), kw(self.kws.vm_error)),
                    (kw(self.kws.kind), kw(self.kws.kinds[*kind as usize])),
                    (kw(self.kws.message), text(msg)),
                ],
                Fault::Io {
                    op,
                    path,
                    kind,
                    msg,
                } => {
                    let mut v = vec![
                        (kw(self.kws.type_), kw(self.kws.io_error)),
                        (kw(self.kws.operation), kw(self.kws.io_ops[*op as usize])),
                    ];
                    // Present only when the operation names one (ADR-042 part
                    // 1): a `:path` of nil on every stdio failure is a key that
                    // says nothing, carried so the count stays even.
                    if let Some(p) = path {
                        v.push((kw(self.kws.path), text(p)));
                    }
                    v.push((kw(self.kws.kind), kw(self.kws.io_kinds[*kind as usize])));
                    v.push((kw(self.kws.message), text(msg)));
                    v
                }
            };
            Value::Map(Rc::new(MapObj(pairs)))
        }
    }

    /// Give a fault its position and make it an unwind. Every VM-raised failure
    /// goes through here, which is what makes "a fault is a throw" one line
    /// rather than a claim (ADR-039 clause 2).
    fn raise(vm: &Vm, kind: Kind, at: SpanOrigin, msg: impl Into<String>) -> Unwind {
        Unwind::new(vm.fault_value(&fault(kind, msg)), at)
    }

    impl Vm {
        /// A fresh symbol name. Deterministic within a compilation unit and
        /// reset between units, which is what keeps `.expanded` goldens from
        /// flapping (BUILD.md, determinism).
        pub fn gensym_name(&mut self, prefix: &str) -> String {
            self.gensym += 1;
            format!("{prefix}__{}", self.gensym)
        }

        pub fn reset_gensym(&mut self) {
            self.gensym = 0;
        }

        /// The buffered host's only write path (ADR-029: emitted effects are
        /// part of the comparison, not something that escapes it).
        pub fn emit(&mut self, text: &str) {
            self.out.push_str(text);
        }

        pub fn take_output(&mut self) -> String {
            std::mem::take(&mut self.out)
        }

        pub fn live_cells(&self) -> usize {
            self.cells.len()
        }

        pub fn new_cell(&mut self, value: Value) -> CellId {
            self.cells.push(CellEntry {
                generation: 0,
                value,
            });
            CellId((self.cells.len() - 1) as u32, 0)
        }

        pub fn cell(&self, id: CellId) -> Option<&Value> {
            let e = self.cells.get(id.0 as usize)?;
            (e.generation == id.1).then_some(&e.value)
        }

        fn set_cell(&mut self, id: CellId, value: Value) -> bool {
            match self.cells.get_mut(id.0 as usize) {
                Some(e) if e.generation == id.1 => {
                    e.value = value;
                    true
                }
                _ => false,
            }
        }

        /// Take an index from the free list before growing, so a program that
        /// opens and closes in a loop occupies a bounded number of slots.
        pub fn open_handle(&mut self, host: crate::host::Host) -> HandleId {
            match self.free_handles.pop() {
                Some(i) => {
                    let e = &mut self.handles[i as usize];
                    // Bumped on *reuse*, never on release. Bumping at `close`
                    // would make the id that just closed the resource stale as
                    // a side effect, so closing twice — exactly what a correct
                    // `with-open` does when the body closed explicitly — would
                    // report the aliasing error instead of being a no-op. The
                    // two halves of ADR-042 part 4 are only compatible this
                    // way round, and the in-language suite caught it wrong.
                    e.generation += 1;
                    e.host = Some(host);
                    HandleId(i, e.generation)
                }
                None => {
                    self.handles.push(HandleEntry {
                        generation: 0,
                        host: Some(host),
                    });
                    HandleId((self.handles.len() - 1) as u32, 0)
                }
            }
        }

        /// `None` for a closed *or* stale handle. Everything that reads or
        /// writes wants exactly this, because both are `:closed` to a program —
        /// the two only differ for `close` itself.
        pub fn host_mut(&mut self, id: HandleId) -> Option<&mut crate::host::Host> {
            let e = self.handles.get_mut(id.0 as usize)?;
            (e.generation == id.1).then_some(e.host.as_mut()?)
        }

        pub fn host(&self, id: HandleId) -> Option<&crate::host::Host> {
            let e = self.handles.get(id.0 as usize)?;
            (e.generation == id.1).then_some(e.host.as_ref()?)
        }

        /// ADR-042 part 4. `false` means *stale*, not *already closed*:
        /// closing twice is what a correct `with-open` does when the body
        /// closed explicitly, and erroring there would make the safe idiom the
        /// dangerous one. A stale handle is a live resource addressed through a
        /// dead name, which is the bug the generation exists to catch.
        pub fn close_handle(&mut self, id: HandleId) -> bool {
            let Some(e) = self.handles.get_mut(id.0 as usize) else {
                return false;
            };
            if e.generation != id.1 {
                return false;
            }
            // Only a slot that actually held something goes back on the free
            // list — otherwise closing twice would queue the index twice, and
            // the second reuse would hand out an id aliasing the first.
            if e.host.take().is_some() {
                self.free_handles.push(id.0);
            }
            true
        }

        /// ADR-029 refuses a snapshot while a handle is live, so milestone 8
        /// asks this once per `Image` — a subtraction rather than a scan.
        pub fn open_handles(&self) -> usize {
            self.handles.len() - self.free_handles.len()
        }

        pub fn global(&self, name: SymId) -> Option<&Value> {
            let id = (*self.globals.get(name.0 as usize)?)?;
            self.cell(id)
        }

        /// ADR-027's create-or-rebind. The name's cell is created once and kept,
        /// so a closure that already read the global keeps seeing rebinds.
        pub fn set_global(&mut self, name: SymId, value: Value) {
            let i = name.0 as usize;
            if self.globals.len() <= i {
                self.globals.resize(i + 1, None);
            }
            match self.globals[i] {
                Some(id) => {
                    self.set_cell(id, value);
                }
                None => {
                    let id = self.new_cell(value);
                    self.globals[i] = Some(id);
                }
            }
        }

        /// Bind a name to a value that is not a function. `io/stdout` is one:
        /// ADR-038 made a primitive an ordinary global, which means a primitive
        /// does not have to be callable to be one.
        pub fn set_named_global(&mut self, name: &str, value: Value) {
            let sym = SymId(self.interner.intern(name));
            self.set_global(sym, value);
        }

        /// Register a primitive as an ordinary global (ADR-038). The `prim`
        /// module is the only caller; the VM has no opinion about which
        /// functions exist (ADR-013).
        pub fn native(&mut self, name: &str, min: u32, variadic: bool, f: NativeFn) {
            let sym = SymId(self.interner.intern(name));
            self.natives.push(Native {
                name: sym,
                min,
                variadic,
                f,
            });
            let id = NativeId((self.natives.len() - 1) as u32);
            self.set_global(sym, Value::Fn(Rc::new(Closure::Native(id))));
        }
    }

    impl Default for Vm {
        fn default() -> Vm {
            Vm::new()
        }
    }

    /// One function activation. Slots live in a single flat `Vec` on the
    /// `Execution`, base-relative, which is the shape an `Image` wants
    /// (ADR-029) and what makes a tail call a truncation rather than a copy.
    pub(crate) struct Frame {
        pub(crate) proto: u32,
        pub(crate) pc: usize,
        /// Index in `Execution::slots` of this frame's slot 0.
        pub(crate) base: usize,
        /// Absolute slot index, in the *caller's* frame, for the return value.
        /// Always below `base`, so returning can truncate first and write after.
        pub(crate) dst: usize,
        /// The slot stack's length before this frame existed. Returning
        /// restores it. Truncating to `base` instead would discard the part of
        /// the *caller's* frame that sits above the call window, which is most
        /// of it — the window is allocated early and the caller keeps using
        /// slots above it after the call returns.
        pub(crate) ret_len: usize,
        pub(crate) closure: Rc<Closure>,
    }

    /// One active `try` region (ADR-028). `err` is the whole difference between
    /// the two kinds: a catch has a slot to bind the thrown value to, a finally
    /// has nothing to bind.
    pub(crate) struct Handler {
        /// The frame that owns the record. Unwinding to it drops every frame
        /// above, which is what makes a handler survive a call.
        pub(crate) frame: usize,
        pub(crate) target: usize,
        pub(crate) err: Option<Slot>,
    }

    /// An unwind parked while a cleanup runs. `depth` is the handler depth the
    /// cleanup body runs at: an unwind that escapes *below* it displaces this
    /// one (ADR-028 invariant 3), and one that does not is a throw the cleanup
    /// caught itself.
    pub(crate) struct Pending {
        pub(crate) depth: usize,
        pub(crate) unwind: Unwind,
    }

    /// `pub(crate)` throughout rather than private: `image` is the one other
    /// module that has to read all of this, and an `Image` that could only see
    /// what an accessor exposed would be an `Image` that silently omits
    /// whatever nobody wrote an accessor for — which is the ADR-005 failure
    /// ADR-029 exists to correct.
    pub struct Execution {
        pub(crate) frames: Vec<Frame>,
        pub(crate) slots: Vec<Value>,
        /// ADR-028: active handlers and finalizers live in VM-owned memory,
        /// reachable from the image — never on the Rust stack.
        pub(crate) handlers: Vec<Handler>,
        pub(crate) pending: Vec<Pending>,
        /// Instructions left before this run suspends (ADR-043 part 4).
        /// `u64::MAX` is the un-fuelled run, which is every caller that is not
        /// the snapshot property.
        pub(crate) fuel: u64,
    }

    impl Execution {
        pub(crate) fn new() -> Execution {
            Execution {
                frames: Vec::new(),
                slots: Vec::new(),
                handlers: Vec::new(),
                pending: Vec::new(),
                fuel: u64::MAX,
            }
        }

        /// The deepest the frame stack reached, and the largest the slot stack
        /// grew. A tail loop keeps both flat, which is milestone 3's exit
        /// condition and not otherwise observable from outside.
        pub fn depth(&self) -> usize {
            self.frames.len()
        }

        pub fn slot_count(&self) -> usize {
            self.slots.len()
        }
    }

    pub fn run(vm: &mut Vm, chunk: &Chunk) -> Outcome {
        run_traced(vm, chunk).0
    }

    /// `run`, plus the high-water marks. Separate because the marks exist for
    /// the constant-space property and have no place in the driver.
    pub fn run_traced(vm: &mut Vm, chunk: &Chunk) -> (Outcome, (usize, usize)) {
        let (outcome, _, peak) = run_fueled(vm, chunk, start(chunk), u64::MAX);
        (outcome, peak)
    }

    /// A program's `Execution` before its first instruction: the top-level
    /// proto in frame 0, nothing else. Separate from running it because
    /// milestone 8 needs to run a program in more than one sitting.
    pub fn start(chunk: &Chunk) -> Execution {
        start_at(chunk, 0)
    }

    /// The same, starting at a named proto rather than at the top of the file.
    /// ADR-044 part 2: a REPL session's chunk has one top-level proto per
    /// input, and evaluating input *n* means running the one input *n* added.
    pub fn start_at(chunk: &Chunk, proto: u32) -> Execution {
        let top = Rc::new(Closure::Fn {
            proto,
            captures: Rc::from(Vec::new()),
        });
        let mut ex = Execution::new();
        ex.slots
            .resize(chunk.protos[proto as usize].slots as usize, Value::Nil);
        ex.frames.push(Frame {
            proto,
            pc: 0,
            base: 0,
            dst: 0,
            ret_len: 0,
            closure: top,
        });
        ex
    }

    /// Run — or resume — for at most `fuel` instructions, handing the
    /// `Execution` back either way. Resuming is calling this again with what it
    /// returned; there is no separate resume path, because a second one would
    /// be a second thing to keep in agreement (ADR-039's argument, applied to
    /// the other end of the loop).
    pub fn run_fueled(
        vm: &mut Vm,
        chunk: &Chunk,
        mut ex: Execution,
        fuel: u64,
    ) -> (Outcome, Execution, (usize, usize)) {
        ex.fuel = fuel;
        let (outcome, peak) = drive_ex(vm, chunk, &mut ex);
        (outcome, ex, peak)
    }

    /// Call a closure from Rust, inside the chunk it was compiled in.
    ///
    /// The chunk is a parameter and not a detail: a `Closure` names its proto by
    /// index (ADR-034), so it means nothing without the chunk those indices are
    /// into. The expander therefore keeps a macro's chunk beside the macro
    /// (ADR-040), and this is the only way into the VM that is not "run a
    /// program from the top".
    pub fn call_in(
        vm: &mut Vm,
        chunk: &Chunk,
        f: Value,
        args: &[Value],
        at: SpanOrigin,
    ) -> Result<Value, Unwind> {
        let mut ex = Execution::new();
        // The callee and its arguments in the window the call protocol expects
        // (ADR-034): callee at `base`, arguments directly above it.
        ex.slots.push(f);
        ex.slots.extend(args.iter().cloned());
        match call(vm, &mut ex, chunk, 0, args.len() as u32, 0, at)? {
            // A native answers without a frame; there is nothing to drive.
            Some(v) => Ok(v),
            None => match drive(vm, chunk, ex).0 {
                Outcome::Returned(v) => Ok(v),
                Outcome::Threw(u) => Err(u),
                // ADR-029 permits a snapshot only at an instruction boundary in
                // compiled code, and never mid-expansion. `call_in` is how a
                // macro body runs, and it always runs un-fuelled — so this arm
                // is where that promise is kept rather than described.
                Outcome::Suspended => unreachable!("`call_in` runs un-fuelled"),
            },
        }
    }

    /// The dispatch loop, over an `Execution` someone else set up.
    fn drive(vm: &mut Vm, chunk: &Chunk, mut ex: Execution) -> (Outcome, (usize, usize)) {
        drive_ex(vm, chunk, &mut ex)
    }

    fn drive_ex(vm: &mut Vm, chunk: &Chunk, ex: &mut Execution) -> (Outcome, (usize, usize)) {
        let mut peak = (ex.frames.len(), ex.slots.len());
        loop {
            // Before the fetch, so the suspension point is *between* two
            // instructions and never inside one. Resuming re-enters here with
            // the same `pc`, which is why `Suspended` needs to carry nothing.
            if ex.fuel == 0 {
                return (Outcome::Suspended, peak);
            }
            ex.fuel -= 1;
            let fi = ex.frames.len() - 1;
            let (pidx, pc) = {
                let f = &ex.frames[fi];
                (f.proto, f.pc)
            };
            let proto = &chunk.protos[pidx as usize];
            let ins = proto.code[pc];
            let at = proto.lines[pc];
            ex.frames[fi].pc = pc + 1;

            match exec(vm, ex, chunk, ins, at) {
                Ok(Step::Next) => {}
                Ok(Step::Done(v)) => {
                    // A record left open by a return is a compiler bug, not a
                    // program error (ADR-028 rule 5). Checked rather than
                    // asserted in a comment: dropping the `POPHANDLER` on the
                    // untroubled path is the first mutation anyone would try.
                    assert!(
                        ex.handlers.is_empty() && ex.pending.is_empty(),
                        "the run finished with {} handler(s) and {} parked unwind(s) open",
                        ex.handlers.len(),
                        ex.pending.len()
                    );
                    return (Outcome::Returned(v), peak);
                }
                // ADR-039: one failure path. A fault and a `throw` arrive here
                // identically, and unwinding is what runs the cleanups.
                Err(u) => {
                    if let Some(escaped) = unwind(ex, u) {
                        return (Outcome::Threw(escaped), peak);
                    }
                }
            }
            peak.0 = peak.0.max(ex.frames.len());
            peak.1 = peak.1.max(ex.slots.len());
        }
    }

    /// What executing one instruction did: carried on, or finished the run by
    /// returning from the outermost frame.
    enum Step {
        Next,
        Done(Value),
    }

    /// One instruction. Split out of the loop so the fallible arms can use `?`
    /// and so an unwind is born in exactly one place.
    fn exec(
        vm: &mut Vm,
        ex: &mut Execution,
        chunk: &Chunk,
        ins: Instr,
        at: SpanOrigin,
    ) -> Result<Step, Unwind> {
        let fi = ex.frames.len() - 1;
        let base = ex.frames[fi].base;
        let proto = &chunk.protos[ex.frames[fi].proto as usize];
        match ins {
            Instr::Const { dst, k } => {
                ex.slots[base + dst as usize] = proto.consts[k as usize].clone();
            }
            Instr::Move { dst, src } => {
                ex.slots[base + dst as usize] = ex.slots[base + src as usize].clone();
            }
            Instr::GetGlobal { dst, name } => {
                let v = vm.global(name).cloned().ok_or_else(|| {
                    raise(
                        vm,
                        Kind::Unbound,
                        at,
                        format!("`{}` is not bound", vm.interner.name(name.0)),
                    )
                })?;
                ex.slots[base + dst as usize] = v;
            }
            Instr::SetGlobal { name, src } => {
                let v = ex.slots[base + src as usize].clone();
                vm.set_global(name, v);
            }
            Instr::GetCapture { dst, idx } => {
                let v = match &*ex.frames[fi].closure {
                    Closure::Fn { captures, .. } => captures[idx as usize].clone(),
                    Closure::Native(_) => {
                        return Err(raise(
                            vm,
                            Kind::Internal,
                            at,
                            "a native function has no captures",
                        ))
                    }
                };
                ex.slots[base + dst as usize] = v;
            }
            Instr::GetSelf { dst } => {
                let me = ex.frames[fi].closure.clone();
                ex.slots[base + dst as usize] = Value::Fn(me);
            }
            Instr::SetCell { cell, src } => {
                let target = ex.slots[base + cell as usize].clone();
                let v = ex.slots[base + src as usize].clone();
                match target {
                    Value::Cell(id) if vm.set_cell(id, v) => {}
                    Value::Cell(_) => {
                        return Err(raise(vm, Kind::Internal, at, "cell is no longer live"))
                    }
                    other => {
                        return Err(raise(
                            vm,
                            Kind::Type,
                            at,
                            format!(
                                "`set-cell!` needs a cell, not a {}",
                                crate::value::kind_name(&other)
                            ),
                        ))
                    }
                }
            }
            Instr::Closure { dst, proto: p } => {
                // ADR-002: captures are copied out of this frame now, not
                // referenced. The descriptors are read here because they
                // name slots in the *enclosing* frame.
                let target = &chunk.protos[p as usize];
                let mut captured = Vec::with_capacity(target.captures.len());
                for c in &target.captures {
                    captured.push(match *c {
                        CaptureSrc::Local(s) => ex.slots[base + s as usize].clone(),
                        CaptureSrc::Capture(i) => match &*ex.frames[fi].closure {
                            Closure::Fn { captures, .. } => captures[i as usize].clone(),
                            Closure::Native(_) => {
                                return Err(raise(
                                    vm,
                                    Kind::Internal,
                                    at,
                                    "a native function has no captures",
                                ))
                            }
                        },
                        CaptureSrc::SelfFn => Value::Fn(ex.frames[fi].closure.clone()),
                    });
                }
                ex.slots[base + dst as usize] = Value::Fn(Rc::new(Closure::Fn {
                    proto: p,
                    captures: Rc::from(captured),
                }));
            }
            Instr::Jump { target } => {
                ex.frames[fi].pc = target as usize;
            }
            Instr::JumpUnless { cond, target } => {
                // Only nil and false are falsy (`TRAPS.md`). 0 and "" are
                // truthy, and so is an empty collection.
                let truthy = !matches!(
                    ex.slots[base + cond as usize],
                    Value::Nil | Value::Bool(false)
                );
                if !truthy {
                    ex.frames[fi].pc = target as usize;
                }
            }
            Instr::Call {
                dst,
                base: cb,
                argc,
            } => {
                let callee = base + cb as usize;
                if let Some(v) = call(vm, ex, chunk, callee, argc, base + dst as usize, at)? {
                    // A native returns immediately; there is no frame to
                    // push and nothing to resume.
                    ex.slots[base + dst as usize] = v;
                }
            }
            Instr::TailCall { base: cb, argc } => {
                let callee = base + cb as usize;
                if let Some(v) = tail_call(vm, ex, chunk, callee, argc, at)? {
                    if let Some(done) = ret(ex, v) {
                        return Ok(Step::Done(done));
                    }
                }
            }
            Instr::Return { src } => {
                let v = ex.slots[base + src as usize].clone();
                if let Some(done) = ret(ex, v) {
                    return Ok(Step::Done(done));
                }
            }
            Instr::Throw { src } => {
                return Err(Unwind::new(ex.slots[base + src as usize].clone(), at));
            }
            // The four handler instructions. A record is pushed here, and
            // removed either by the `POPHANDLER` on the path where nothing
            // was thrown or by `unwind` dispatching to it — never both,
            // which is ADR-028 invariant 1 read off the bytecode.
            Instr::PushHandler { catch, err } => ex.handlers.push(Handler {
                frame: fi,
                target: catch as usize,
                err: Some(err),
            }),
            Instr::PushFinally { target } => ex.handlers.push(Handler {
                frame: fi,
                target: target as usize,
                err: None,
            }),
            Instr::PopHandler => {
                let h = ex
                    .handlers
                    .pop()
                    .expect("POPHANDLER with no open handler region");
                debug_assert_eq!(h.frame, fi, "a handler record outlived its frame");
            }
            Instr::EndFinally => {
                // Only the unwinding copy of a cleanup ends here (ADR-034),
                // so there is always something parked: pick it back up and
                // carry on unwinding.
                let p = ex
                    .pending
                    .pop()
                    .expect("ENDFINALLY outside an unwinding cleanup");
                return Err(p.unwind);
            }
        }
        Ok(Step::Next)
    }

    /// Deliver an unwind to the innermost handler record, or hand it back when
    /// the handler stack is empty and the run is over.
    ///
    /// Exactly one record is popped, because every record is *entered* when
    /// unwinding reaches it: a catch binds the value, a finally parks it and
    /// runs the cleanup. Nothing here searches for a matching handler — there is
    /// no filter to match on (ADR-039 clause 5).
    fn unwind(ex: &mut Execution, mut u: Unwind) -> Option<Unwind> {
        let h = match ex.handlers.pop() {
            Some(h) => h,
            None => {
                // Nothing left to catch it, so every parked error it displaced
                // is retained on it (ADR-028 invariant 3).
                while let Some(p) = ex.pending.pop() {
                    u.suppress(p.unwind);
                }
                return Some(u);
            }
        };
        // A parked unwind whose cleanup this one has escaped is displaced by it.
        // One that is caught *inside* the cleanup never unwinds below the depth
        // that cleanup runs at, so it leaves the parked error alone.
        while ex
            .pending
            .last()
            .is_some_and(|p| p.depth > ex.handlers.len())
        {
            let p = ex.pending.pop().expect("just checked");
            u.suppress(p.unwind);
        }
        // Frames above the record's owner are gone, through the same call a
        // normal return uses — see `drop_frame` for why that matters.
        while ex.frames.len() > h.frame + 1 {
            drop_frame(ex);
        }
        let base = ex.frames[h.frame].base;
        ex.frames[h.frame].pc = h.target;
        match h.err {
            // The handler binds the value alone. Position and the suppressed
            // chain end here, by decision (ADR-039 clause 4).
            Some(slot) => ex.slots[base + slot as usize] = u.value,
            None => ex.pending.push(Pending {
                depth: ex.handlers.len(),
                unwind: u,
            }),
        }
        None
    }

    /// Pop one frame and give its slots back.
    ///
    /// The only place a frame is released, so returning and unwinding cannot
    /// disagree about what that means — and the milestone-4 mutation pass says
    /// that sharing is the *only* thing keeping the unwinding side honest.
    /// Dropping the frames while keeping their slots leaves every test green:
    /// the leak is bounded, never reaches a value, and cannot move a high-water
    /// mark. It is visible only as dead slots carried into later frames and
    /// into an `Image` (ADR-029, `notes/milestone-4-mutants.md`).
    fn drop_frame(ex: &mut Execution) -> Frame {
        let f = ex.frames.pop().expect("a frame to drop");
        ex.slots.truncate(f.ret_len);
        f
    }

    /// Pop the finished frame and deliver its value. `None` means execution
    /// continues; `Some` means the outermost frame returned.
    fn ret(ex: &mut Execution, value: Value) -> Option<Value> {
        let f = drop_frame(ex);
        if ex.frames.is_empty() {
            return Some(value);
        }
        ex.slots[f.dst] = value;
        None
    }

    /// Push a frame for a bytecode callee, or run a native and hand back its
    /// value. `Ok(None)` means a frame was pushed.
    fn call(
        vm: &mut Vm,
        ex: &mut Execution,
        chunk: &Chunk,
        callee: usize,
        argc: u32,
        dst: usize,
        at: SpanOrigin,
    ) -> Result<Option<Value>, Unwind> {
        match callee_at(vm, ex, callee, at)? {
            Callee::Native(id) => native_call(vm, ex, id, callee, argc, at).map(Some),
            Callee::Bytecode(proto, c) => {
                // The arguments already sit at `callee+1..`, which is exactly
                // where the callee's slots 0.. must be — so a call copies
                // nothing (ADR-034).
                let base = callee + 1;
                let ret_len = ex.slots.len();
                enter(vm, ex, chunk, proto, argc, base, at)?;
                ex.frames.push(Frame {
                    proto,
                    pc: 0,
                    base,
                    dst,
                    ret_len,
                    closure: c,
                });
                Ok(None)
            }
        }
    }

    /// ADR-028: reuse the caller's frame. The arguments move down to `base`,
    /// which is what keeps a tail loop in constant space.
    fn tail_call(
        vm: &mut Vm,
        ex: &mut Execution,
        chunk: &Chunk,
        callee: usize,
        argc: u32,
        at: SpanOrigin,
    ) -> Result<Option<Value>, Unwind> {
        match callee_at(vm, ex, callee, at)? {
            Callee::Native(id) => native_call(vm, ex, id, callee, argc, at).map(Some),
            Callee::Bytecode(proto, c) => {
                let fi = ex.frames.len() - 1;
                let base = ex.frames[fi].base;
                // Copying ascending is safe: `base <= callee`, so the
                // destination of each argument is always below its source.
                for i in 0..argc as usize {
                    ex.slots[base + i] = ex.slots[callee + 1 + i].clone();
                }
                enter(vm, ex, chunk, proto, argc, base, at)?;
                ex.frames[fi].proto = proto;
                ex.frames[fi].pc = 0;
                ex.frames[fi].closure = c;
                Ok(None)
            }
        }
    }

    /// What is in the callee slot, resolved once so `call` and `tail_call`
    /// share one answer and one error message.
    enum Callee {
        Native(NativeId),
        Bytecode(u32, Rc<Closure>),
    }

    fn callee_at(vm: &Vm, ex: &Execution, callee: usize, at: SpanOrigin) -> Result<Callee, Unwind> {
        match &ex.slots[callee] {
            Value::Fn(c) => Ok(match &**c {
                Closure::Native(id) => Callee::Native(*id),
                Closure::Fn { proto, .. } => Callee::Bytecode(*proto, c.clone()),
            }),
            other => Err(raise(
                vm,
                Kind::NotCallable,
                at,
                format!("cannot call a {}", crate::value::kind_name(other)),
            )),
        }
    }

    /// The callee prologue: check arity once, at call time (ADR-033 rule 2),
    /// size the frame, and pack a rest parameter (ADR-038).
    fn enter(
        vm: &Vm,
        ex: &mut Execution,
        chunk: &Chunk,
        proto: u32,
        argc: u32,
        base: usize,
        at: SpanOrigin,
    ) -> Result<(), Unwind> {
        let p = &chunk.protos[proto as usize];
        let name = match p.name {
            Some(s) => vm.interner.name(s.0).to_string(),
            None => "fn".to_string(),
        };
        let fixed = if p.variadic { p.params - 1 } else { p.params };
        if argc < fixed || (!p.variadic && argc > fixed) {
            let wanted = if p.variadic {
                format!("at least {fixed}")
            } else {
                fixed.to_string()
            };
            return Err(raise(
                vm,
                Kind::Arity,
                at,
                format!("`{name}` takes {wanted} argument(s), given {argc}"),
            ));
        }
        // ADR-038: whatever the call actually needs, since ADR-034 sets no
        // maximum arity and the compiler therefore cannot have reserved it.
        let want = (p.slots as usize).max(argc as usize);
        if ex.slots.len() < base + want {
            ex.slots.resize(base + want, Value::Nil);
        }
        // Everything above the arguments starts as nil. The compiler writes
        // every slot before it reads one, so this is not needed for
        // correctness — it is needed so a frame's contents are a function of
        // the call and not of whatever ran here last, which is what an `Image`
        // has to serialize (ADR-029).
        for s in ex
            .slots
            .iter_mut()
            .skip(base + argc as usize)
            .take(want.saturating_sub(argc as usize))
        {
            *s = Value::Nil;
        }
        if p.variadic {
            // Packed in place: slots `fixed..argc` are dead the instant they are
            // collected, because the prologue runs before the first
            // instruction. The rest is an empty *list*, never nil (ADR-033,
            // E-11).
            let rest: Vec<Value> = ex.slots[base + fixed as usize..base + argc as usize].to_vec();
            for s in ex
                .slots
                .iter_mut()
                .skip(base + fixed as usize)
                .take(want - fixed as usize)
            {
                *s = Value::Nil;
            }
            ex.slots[base + fixed as usize] = Value::List(Rc::new(ListObj(rest)));
        }
        Ok(())
    }

    fn native_call(
        vm: &mut Vm,
        ex: &Execution,
        id: NativeId,
        callee: usize,
        argc: u32,
        at: SpanOrigin,
    ) -> Result<Value, Unwind> {
        let n = &vm.natives[id.0 as usize];
        let (min, variadic, f, name) = (n.min, n.variadic, n.f, n.name);
        if argc < min || (!variadic && argc > min) {
            let wanted = if variadic {
                format!("at least {min}")
            } else {
                min.to_string()
            };
            return Err(raise(
                vm,
                Kind::Arity,
                at,
                format!(
                    "`{}` takes {wanted} argument(s), given {argc}",
                    vm.interner.name(name.0)
                ),
            ));
        }
        // `ex` and `vm` are distinct, so the arguments are borrowed in place
        // rather than copied into a temporary for every native call.
        let args = &ex.slots[callee + 1..callee + 1 + argc as usize];
        // A native raises a `Fault`, which has no position: it acquires the
        // calling instruction's origin here, the same one a bytecode callee's
        // arity error gets.
        f(vm, args).map_err(|e| Unwind::new(vm.fault_value(&e), at))
    }
}

// ---------------------------------------------------------------------------

/// Macro expansion, quasiquote, and gensym (ADR-024 as amended by ADR-040).
///
/// This is where ADR-004's predicted coupling arrives: a macro is language
/// code, so expanding a form means *running* one — compiling the macro's
/// function, calling it with the unexpanded argument forms as values, and
/// walking whatever it hands back. Compilation therefore stops being a pure
/// function of source, exactly as that entry said it would.
///
/// The macro table lives here rather than on the `Vm`, because a macro is a
/// property of a compilation unit and not of the machine. Nothing about it
/// reaches an `Image` (ADR-029), and two units compiled by one VM cannot see
/// each other's macros.
pub mod expand {
    use crate::bytecode::Chunk;
    use crate::compile;
    use crate::error::{LispErr, SpanOrigin};
    use crate::printer;
    use crate::reader::{self, MAX_NESTING};
    use crate::value::{Interner, ListObj, LocatedForm, MapObj, Origins, SymId, Value, VecObj};
    use crate::vm::{self, Outcome, Vm};
    use std::collections::HashMap;
    use std::rc::Rc;

    /// `def` and `defmacro`, written in the language (ADR-027, ADR-040).
    const PRELUDE: &str = include_str!("prelude.xs");

    /// A macro that rewrites to itself makes no progress and would otherwise
    /// hang. The bound is per form and generous; hitting it is a bug in the
    /// macro, and the diagnostic says so.
    const MAX_EXPANSIONS: usize = 512;

    /// The names the expander compares against, interned once. Everything else
    /// it sees is a call or a datum.
    struct Names {
        quote: SymId,
        quasiquote: SymId,
        unquote: SymId,
        unquote_splicing: SymId,
        set_macro: SymId,
        list: SymId,
        concat: SymId,
        vec: SymId,
        vector: SymId,
        hash_map: SymId,
    }

    impl Names {
        fn new(i: &mut Interner) -> Names {
            let mut s = |n: &str| SymId(i.intern(n));
            Names {
                quote: s("quote"),
                quasiquote: s("quasiquote"),
                unquote: s("unquote"),
                unquote_splicing: s("unquote-splicing"),
                set_macro: s("set-macro!"),
                list: s("list"),
                concat: s("concat"),
                vec: s("vec"),
                vector: s("vector"),
                hash_map: s("hash-map"),
            }
        }
    }

    /// A macro is a closure plus the chunk it was compiled in, because a
    /// closure names its proto by index and means nothing without it
    /// (ADR-034).
    struct Macro {
        chunk: Rc<Chunk>,
        f: Value,
    }

    /// The macro table, separated from the expander so it can outlive one
    /// call. A file's table lives for one `expand_all`; a REPL session's lives
    /// for the session, which is ADR-044 part 1 — a `defmacro` typed at the
    /// prompt has to still be there on the next line.
    pub struct Macros {
        /// Looked up, never iterated — so no ordering of this map can reach
        /// output (BUILD.md, determinism).
        table: HashMap<SymId, Macro>,
    }

    impl Macros {
        /// A table with the prelude already in it. Expanding the prelude is
        /// once per *unit*, and ADR-044 makes a session one unit — so a session
        /// pays for it once rather than once per input.
        pub fn with_prelude(vm: &mut Vm) -> Macros {
            let mut m = Macros {
                table: HashMap::new(),
            };
            let names = Names::new(&mut vm.interner);
            let mut ex = Expander {
                vm,
                names,
                macros: &mut m.table,
                depth: 0,
            };
            ex.prelude();
            m
        }
    }

    struct Expander<'a> {
        vm: &'a mut Vm,
        names: Names,
        macros: &'a mut HashMap<SymId, Macro>,
        depth: usize,
    }

    /// Expand one compilation unit: the prelude first, then the unit's own
    /// forms in order.
    ///
    /// Order is load-bearing and is ADR-033's sequential top level applied one
    /// phase earlier: a `set-macro!` has to have run before a later form can be
    /// a call to it.
    pub fn expand_all(forms: Vec<LocatedForm>, vm: &mut Vm) -> Result<Vec<LocatedForm>, LispErr> {
        // Per compilation unit, so the same source expands the same way twice
        // and a golden can pin it (BUILD.md, determinism). A file is a unit; a
        // REPL session is also a unit, which is why `expand_in` does *not* do
        // this (ADR-044 part 1).
        vm.reset_gensym();
        let mut macros = Macros::with_prelude(vm);
        expand_in(forms, vm, &mut macros)
    }

    /// Expand into an existing macro table, and do not touch the gensym
    /// counter.
    ///
    /// Both omissions are the same decision. A REPL session is one unit, so its
    /// macros accumulate across inputs and its counter runs monotonically to
    /// the end of the session — a counter that restarted per input could mint a
    /// name it had already handed out, which is the one thing a fresh symbol
    /// may not do.
    pub fn expand_in(
        forms: Vec<LocatedForm>,
        vm: &mut Vm,
        macros: &mut Macros,
    ) -> Result<Vec<LocatedForm>, LispErr> {
        let names = Names::new(&mut vm.interner);
        let mut ex = Expander {
            vm,
            names,
            macros: &mut macros.table,
            depth: 0,
        };
        forms.into_iter().map(|f| ex.form(f)).collect()
    }

    impl Expander<'_> {
        /// The prelude is our own file, so a failure in it is a host bug rather
        /// than a diagnostic for the user (ADR-028 rule 5's spirit). Its output
        /// forms are discarded: everything it does, it does by installing
        /// macros.
        fn prelude(&mut self) {
            let forms = reader::read_all(PRELUDE, &mut self.vm.interner).unwrap_or_else(|e| {
                panic!("prelude does not read: {}", e.render("prelude.xs", PRELUDE))
            });
            for f in forms {
                self.form(f).unwrap_or_else(|e| {
                    panic!(
                        "prelude does not expand: {}",
                        e.render("prelude.xs", PRELUDE)
                    )
                });
            }
        }

        /// Expand one form to a fixed point, then its children.
        ///
        /// ADR-036 puts the nesting bound in the reader and gives this phase the
        /// forms *it* produces — the second entry point that entry allows, not a
        /// second mechanism. The counter is here rather than in the walk because
        /// macro output is where unbounded nesting can appear without any source
        /// text to have been read.
        fn form(&mut self, f: LocatedForm) -> Result<LocatedForm, LispErr> {
            if self.depth >= MAX_NESTING {
                return Err(LispErr::at_origin(
                    f.origins.origin,
                    format!("expansion nested more than {MAX_NESTING} deep (ADR-036)"),
                ));
            }
            self.depth += 1;
            let out = self.form_inner(f);
            self.depth -= 1;
            out
        }

        fn form_inner(&mut self, f: LocatedForm) -> Result<LocatedForm, LispErr> {
            let mut f = f;
            for _ in 0..MAX_EXPANSIONS {
                let head = match head_sym(&f.root) {
                    None => return self.children(f),
                    Some(h) => h,
                };
                // `quote` is the one head whose contents are data rather than
                // code. Descending into it would expand a macro call the
                // program is only talking *about*.
                if head == self.names.quote {
                    return Ok(f);
                }
                if head == self.names.quasiquote {
                    let out = self.quasiquote(&f)?;
                    return self.form(out);
                }
                if head == self.names.unquote || head == self.names.unquote_splicing {
                    return Err(LispErr::at_origin(
                        f.origins.origin,
                        format!("`{}` outside a quasiquote", self.vm.interner.name(head.0)),
                    ));
                }
                if head == self.names.set_macro {
                    return self.set_macro(f);
                }
                match self.macros.contains_key(&head) {
                    false => return self.children(f),
                    true => f = self.invoke(head, &f)?,
                }
            }
            Err(LispErr::at_origin(
                f.origins.origin,
                format!("expanded {MAX_EXPANSIONS} times without settling — a macro is rewriting to itself"),
            ))
        }

        /// Rebuild an aggregate with expanded children, keeping origins paired
        /// with the nodes they describe.
        fn children(&mut self, f: LocatedForm) -> Result<LocatedForm, LispErr> {
            let LocatedForm { root, origins } = f;
            let Origins { origin, children } = origins;
            let (root, children) = match root {
                Value::List(l) => {
                    let (vs, os) = self.each(l.0.clone(), children)?;
                    (Value::List(Rc::new(ListObj(vs))), os)
                }
                Value::Vec(x) => {
                    let (vs, os) = self.each(x.0.clone(), children)?;
                    (Value::Vec(Rc::new(VecObj(vs))), os)
                }
                Value::Map(m) => {
                    let flat: Vec<Value> =
                        m.0.iter()
                            .flat_map(|(k, v)| [k.clone(), v.clone()])
                            .collect();
                    let (vs, os) = self.each(flat, children)?;
                    let pairs = vs.chunks(2).map(|p| (p[0].clone(), p[1].clone())).collect();
                    (Value::Map(Rc::new(MapObj(pairs))), os)
                }
                other => (other, children),
            };
            Ok(LocatedForm {
                root,
                origins: Origins { origin, children },
            })
        }

        /// Every child of an aggregate, expanded, with its origin travelling
        /// beside it — the pairing is the whole point (ADR-026).
        fn each(
            &mut self,
            items: Vec<Value>,
            origins: Vec<Origins>,
        ) -> Result<(Vec<Value>, Vec<Origins>), LispErr> {
            let mut vs = Vec::with_capacity(items.len());
            let mut os = Vec::with_capacity(items.len());
            for (v, o) in items.into_iter().zip(origins) {
                let out = self.form(LocatedForm {
                    root: v,
                    origins: o,
                })?;
                vs.push(out.root);
                os.push(out.origins);
            }
            Ok((vs, os))
        }

        /// `(set-macro! name expr)` — the one form the expander knows about
        /// defining things (ADR-040). The expression is compiled and run *now*,
        /// and the closure it yields becomes a macro for the rest of the unit.
        ///
        /// The form itself expands to `(quote name)`: it has already had its
        /// entire effect, and leaving it as something with a value keeps the
        /// top level a sequence of expressions.
        fn set_macro(&mut self, f: LocatedForm) -> Result<LocatedForm, LispErr> {
            let items = match &f.root {
                Value::List(l) if l.0.len() == 3 => l.0.clone(),
                _ => {
                    return Err(LispErr::at_origin(
                        f.origins.origin,
                        "`set-macro!` takes a name and an expression",
                    ))
                }
            };
            let name = match items[1] {
                Value::Sym(s) => s,
                ref other => {
                    return Err(LispErr::at_origin(
                        f.origins.children[1].origin,
                        format!(
                            "`set-macro!` needs a name, not a {}",
                            crate::value::kind_name(other)
                        ),
                    ))
                }
            };
            let body = self.form(LocatedForm {
                root: items[2].clone(),
                origins: f.origins.children[2].clone(),
            })?;
            let at = body.origins.origin;
            let chunk = compile::compile(&[body], &mut self.vm.interner)?;
            let value = match vm::run(self.vm, &chunk) {
                Outcome::Suspended => unreachable!("macro expansion runs un-fuelled"),
                Outcome::Returned(v) => v,
                Outcome::Threw(u) => {
                    return Err(LispErr::at_origin(
                        at,
                        format!(
                            "defining macro `{}` threw {}",
                            self.vm.interner.name(name.0),
                            printer::print(&u.value, &self.vm.interner)
                        ),
                    ))
                }
            };
            if !matches!(value, Value::Fn(_)) {
                return Err(LispErr::at_origin(
                    at,
                    format!(
                        "a macro must be a function, not a {}",
                        crate::value::kind_name(&value)
                    ),
                ));
            }
            self.macros.insert(
                name,
                Macro {
                    chunk: Rc::new(chunk),
                    f: value,
                },
            );
            let quote = Value::Sym(self.names.quote);
            Ok(LocatedForm {
                root: Value::List(Rc::new(ListObj(vec![quote, items[1].clone()]))),
                origins: Origins {
                    origin: f.origins.origin,
                    children: vec![f.origins.children[0].clone(), f.origins.children[1].clone()],
                },
            })
        }

        /// Run a macro over its *unexpanded* arguments and give the result
        /// origins (ADR-026): a node identifiable as one of the arguments keeps
        /// its `Source` position, and everything the macro built carries
        /// `Generated(call site)`.
        fn invoke(&mut self, name: SymId, f: &LocatedForm) -> Result<LocatedForm, LispErr> {
            let items = match &f.root {
                Value::List(l) => l.0.clone(),
                _ => unreachable!("a macro call is a list"),
            };
            let args: Vec<Value> = items[1..].to_vec();
            let m = &self.macros[&name];
            let (chunk, closure) = (m.chunk.clone(), m.f.clone());
            let at = f.origins.origin;
            let out = vm::call_in(self.vm, &chunk, closure, &args, at).map_err(|u| {
                LispErr::at_origin(
                    at,
                    format!(
                        "macro `{}` threw {}",
                        self.vm.interner.name(name.0),
                        printer::print(&u.value, &self.vm.interner)
                    ),
                )
            })?;
            // Positions the arguments brought with them, indexed by object
            // identity so a *sub*form the macro passed through keeps its own
            // position and not just a whole argument.
            let mut known = HashMap::new();
            for (v, o) in items[1..].iter().zip(&f.origins.children[1..]) {
                index_origins(v, o, &mut known);
            }
            let generated = match at.span() {
                Some(s) => SpanOrigin::Generated(s),
                // A macro call with no position of its own — expansion of
                // already-generated code. There is nothing better to say.
                None => SpanOrigin::Unknown,
            };
            Ok(LocatedForm {
                origins: origins_for(&out, generated, &known),
                root: out,
            })
        }

        /// Lower `` `x `` to the calls that build it: `list`, `concat`, `vec`,
        /// and `hash-map`, all ordinary globals (ADR-038).
        fn quasiquote(&mut self, f: &LocatedForm) -> Result<LocatedForm, LispErr> {
            let inner = match &f.root {
                Value::List(l) if l.0.len() == 2 => l.0[1].clone(),
                _ => {
                    return Err(LispErr::at_origin(
                        f.origins.origin,
                        "`quasiquote` takes one form",
                    ))
                }
            };
            let at = f.origins.origin;
            let mut auto = HashMap::new();
            let built = self.template(&inner, at, &mut auto)?;
            Ok(LocatedForm {
                origins: generated_origins(&built, at),
                root: built,
            })
        }

        /// One level of template. Returns a form that *constructs* the input.
        fn template(
            &mut self,
            v: &Value,
            at: SpanOrigin,
            auto: &mut HashMap<SymId, Value>,
        ) -> Result<Value, LispErr> {
            match v {
                Value::List(l) => {
                    if let Some(h) = head_sym(v) {
                        if h == self.names.quasiquote {
                            return Err(LispErr::at_origin(
                                at,
                                "a quasiquote inside a quasiquote is not supported (ADR-040)",
                            ));
                        }
                        if h == self.names.unquote {
                            return self.unquoted(l, at);
                        }
                        if h == self.names.unquote_splicing {
                            return Err(LispErr::at_origin(
                                at,
                                "`~@` has nothing to splice into here",
                            ));
                        }
                    }
                    Ok(match self.sequence(&l.0, at, auto)? {
                        Items::Plain(vs) => call(Value::Sym(self.names.list), vs),
                        Items::Spliced(c) => c,
                    })
                }
                // A spliced vector goes through a list, because splicing is a
                // list operation; an unspliced one is a direct `vector` call,
                // because that is what it means and it reads that way in the
                // golden.
                Value::Vec(x) => Ok(match self.sequence(&x.0, at, auto)? {
                    Items::Plain(vs) => call(Value::Sym(self.names.vector), vs),
                    Items::Spliced(c) => call(Value::Sym(self.names.vec), vec![c]),
                }),
                Value::Map(m) => {
                    let flat: Vec<Value> =
                        m.0.iter()
                            .flat_map(|(k, v)| [k.clone(), v.clone()])
                            .collect();
                    match self.sequence(&flat, at, auto)? {
                        Items::Plain(vs) => Ok(call(Value::Sym(self.names.hash_map), vs)),
                        // Splicing pairs into a map needs `apply`, which v1
                        // does not have. Refused rather than half-supported.
                        Items::Spliced(_) => Err(LispErr::at_origin(
                            at,
                            "`~@` inside a map template is not supported (ADR-040)",
                        )),
                    }
                }
                // `x#` is one fresh name per template, so two occurrences in
                // one template are the same symbol and two templates never
                // collide (ADR-040).
                Value::Sym(s) => {
                    let name = self.vm.interner.name(s.0);
                    if name.len() > 1 && name.ends_with('#') {
                        let fresh = match auto.get(s) {
                            Some(v) => v.clone(),
                            None => {
                                let base = name.trim_end_matches('#').to_string();
                                let generated = self.vm.gensym_name(&base);
                                let sym = self.vm.interner.sym(&generated);
                                auto.insert(*s, sym.clone());
                                sym
                            }
                        };
                        return Ok(quoted(Value::Sym(self.names.quote), fresh));
                    }
                    Ok(quoted(Value::Sym(self.names.quote), v.clone()))
                }
                // Everything else evaluates to itself, so quoting it would be
                // noise in every expansion anybody reads. A symbol is the only
                // atom that means something else in code position.
                other => Ok(other.clone()),
            }
        }

        fn unquoted(&mut self, l: &Rc<ListObj>, at: SpanOrigin) -> Result<Value, LispErr> {
            match l.0.len() {
                2 => Ok(l.0[1].clone()),
                _ => Err(LispErr::at_origin(at, "`~` takes one form")),
            }
        }

        /// The items of a template aggregate: either the item forms themselves,
        /// or — once anything is spliced — one form that concatenates the
        /// groups. Which one it is decides how the caller builds its own shape,
        /// and it is the only thing that differs between a list, a vector, and
        /// a map template.
        fn sequence(
            &mut self,
            items: &[Value],
            at: SpanOrigin,
            auto: &mut HashMap<SymId, Value>,
        ) -> Result<Items, LispErr> {
            let mut groups: Vec<Value> = Vec::new();
            let mut plain: Vec<Value> = Vec::new();
            let mut spliced = false;
            for item in items {
                let splice = match head_sym(item) {
                    Some(h) if h == self.names.unquote_splicing => match item {
                        Value::List(l) if l.0.len() == 2 => Some(l.0[1].clone()),
                        _ => return Err(LispErr::at_origin(at, "`~@` takes one form")),
                    },
                    _ => None,
                };
                match splice {
                    Some(e) => {
                        spliced = true;
                        if !plain.is_empty() {
                            groups.push(call(
                                Value::Sym(self.names.list),
                                std::mem::take(&mut plain),
                            ));
                        }
                        groups.push(e);
                    }
                    None => plain.push(self.template(item, at, auto)?),
                }
            }
            if !spliced {
                return Ok(Items::Plain(plain));
            }
            if !plain.is_empty() {
                groups.push(call(Value::Sym(self.names.list), plain));
            }
            Ok(Items::Spliced(call(Value::Sym(self.names.concat), groups)))
        }
    }

    /// What a template's items came to. `Spliced` already carries a form that
    /// builds the whole sequence; `Plain` leaves that to the caller.
    enum Items {
        Plain(Vec<Value>),
        Spliced(Value),
    }

    fn call(head: Value, mut args: Vec<Value>) -> Value {
        let mut items = vec![head];
        items.append(&mut args);
        Value::List(Rc::new(ListObj(items)))
    }

    fn quoted(quote: Value, v: Value) -> Value {
        Value::List(Rc::new(ListObj(vec![quote, v])))
    }

    fn head_sym(v: &Value) -> Option<SymId> {
        match v {
            Value::List(l) => match l.0.first() {
                Some(Value::Sym(s)) => Some(*s),
                _ => None,
            },
            _ => None,
        }
    }

    /// The address of a value's heap object, for recognizing a form the macro
    /// passed through rather than built. Immediates have no identity to
    /// compare, so they are simply not recognized — which costs a position
    /// nobody had a better answer for.
    fn identity(v: &Value) -> Option<usize> {
        match v {
            Value::List(l) => Some(Rc::as_ptr(l) as usize),
            Value::Vec(x) => Some(Rc::as_ptr(x) as usize),
            Value::Map(m) => Some(Rc::as_ptr(m) as usize),
            Value::Str(s) => Some(Rc::as_ptr(s) as usize),
            _ => None,
        }
    }

    fn index_origins(v: &Value, o: &Origins, out: &mut HashMap<usize, Origins>) {
        if let Some(id) = identity(v) {
            out.insert(id, o.clone());
        }
        for (c, co) in crate::value::children(v).iter().zip(&o.children) {
            index_origins(c, co, out);
        }
    }

    /// ADR-026, for macro output: a node the expander can still identify keeps
    /// the position it was read at; everything else is `Generated` at the call
    /// site. Nothing becomes `Unknown` here — a macro call always has a call
    /// site, and reporting it beats reporting nothing.
    fn origins_for(v: &Value, generated: SpanOrigin, known: &HashMap<usize, Origins>) -> Origins {
        if let Some(o) = identity(v).and_then(|id| known.get(&id)) {
            return o.clone();
        }
        Origins {
            origin: generated,
            children: crate::value::children(v)
                .iter()
                .map(|c| origins_for(c, generated, known))
                .collect(),
        }
    }

    /// Origins for a form the expander built out of nothing but the template it
    /// was given: all of it is generated, at the template's own position.
    fn generated_origins(v: &Value, at: SpanOrigin) -> Origins {
        let origin = match at.span() {
            Some(s) => SpanOrigin::Generated(s),
            None => SpanOrigin::Unknown,
        };
        Origins {
            origin,
            children: crate::value::children(v)
                .iter()
                .map(|c| generated_origins(c, at))
                .collect(),
        }
    }
}

// ---------------------------------------------------------------------------

/// The primitive set: values, collections, strings, bytes (ADR-038).
///
/// Separate from `vm` because the two answer different questions. The VM owns
/// the call protocol and has no opinion about which functions exist (ADR-013);
/// this module is that opinion, and cutting it out leaves a machine that runs
/// bytecode and knows no globals.
///
/// Every entry here is an ordinary global, so `+` is a value you can pass to
/// `map` and `set-global!` can rebind any of them — the wart ADR-038 accepts.
pub mod prim {
    use crate::printer;
    use crate::value::{equal, kind_name, BytesObj, ListObj, MapObj, StrObj, Value, VecObj};
    use crate::vm::{fault, Fault, Kind, Vm};
    use std::rc::Rc;

    pub fn install(vm: &mut Vm) {
        // --- numbers (ADR-041 part 3) ---------------------------------------
        vm.native("+", 0, true, |_, a| {
            fold(a, 0, i64::checked_add, |x, y| x + y, "+")
        });
        vm.native("*", 0, true, |_, a| {
            fold(a, 1, i64::checked_mul, |x, y| x * y, "*")
        });
        vm.native("-", 1, true, |_, a| {
            let first = num(&a[0], "-")?;
            if a.len() == 1 {
                return Ok(match first {
                    Num::Int(i) => Value::Int(i.checked_neg().ok_or_else(|| overflow("-"))?),
                    Num::Float(f) => Value::Float(-f),
                });
            }
            let mut acc = first;
            for v in &a[1..] {
                let n = num(v, "-")?;
                acc = match (acc, n) {
                    (Num::Int(x), Num::Int(y)) => {
                        Num::Int(x.checked_sub(y).ok_or_else(|| overflow("-"))?)
                    }
                    _ => Num::Float(as_f64(acc) - as_f64(n)),
                };
            }
            Ok(number(acc))
        });
        // Always a float: there are no ratios, and truncating under the `/`
        // spelling is the silent wrong answer ADR-037 rejected (ADR-041).
        vm.native("/", 1, true, |_, a| {
            let first = as_f64(num(&a[0], "/")?);
            if a.len() == 1 {
                return Ok(Value::Float(1.0 / first));
            }
            let mut acc = first;
            for v in &a[1..] {
                acc /= as_f64(num(v, "/")?);
            }
            Ok(Value::Float(acc))
        });
        vm.native("quot", 2, false, |_, a| int_div(a, "quot"));
        vm.native("rem", 2, false, |_, a| int_div(a, "rem"));
        vm.native("<", 2, false, |_, a| {
            Ok(Value::Bool(compare(&a[0], &a[1], "<")?.is_lt()))
        });
        vm.native(">", 2, false, |_, a| {
            Ok(Value::Bool(compare(&a[0], &a[1], ">")?.is_gt()))
        });
        vm.native("<=", 2, false, |_, a| {
            Ok(Value::Bool(compare(&a[0], &a[1], "<=")?.is_le()))
        });
        vm.native(">=", 2, false, |_, a| {
            Ok(Value::Bool(compare(&a[0], &a[1], ">=")?.is_ge()))
        });
        // Numeric equality, which *does* cross Int and Float — the escape
        // hatch for `=` being type-strict (ADR-041 part 2).
        vm.native("==", 2, false, |_, a| {
            Ok(Value::Bool(compare(&a[0], &a[1], "==")?.is_eq()))
        });

        // --- equality and truth ----------------------------------------------
        vm.native("=", 1, true, |_, a| {
            Ok(Value::Bool(a.windows(2).all(|p| equal(&p[0], &p[1]))))
        });
        vm.native("not=", 1, true, |_, a| {
            Ok(Value::Bool(!a.windows(2).all(|p| equal(&p[0], &p[1]))))
        });
        // Only nil and false are falsy (`TRAPS.md`), and this is the one place
        // that rule exists outside the `JumpUnless` opcode.
        vm.native("not", 1, false, |_, a| {
            Ok(Value::Bool(matches!(a[0], Value::Nil | Value::Bool(false))))
        });

        // --- collections (ADR-041 parts 1 and 4) -----------------------------
        // `nil` reads as the empty thing on the operations that read, and as
        // the empty thing *of the operation's own kind* on the ones that build.
        vm.native("count", 1, false, |_, a| match &a[0] {
            Value::Nil => Ok(Value::Int(0)),
            Value::List(l) => Ok(Value::Int(l.0.len() as i64)),
            Value::Vec(x) => Ok(Value::Int(x.0.len() as i64)),
            Value::Map(m) => Ok(Value::Int(m.0.len() as i64)),
            // ADR-018: a string is not a sequence, and `count` on one is the
            // question "in what unit" wearing a shorter name.
            Value::Str(_) => Err(fault(
                Kind::Type,
                "`count` on a string: say the unit — `str-len` for bytes, \
                 `str-scalars` for scalar values (ADR-018)"
                    .to_string(),
            )),
            other => Err(fault(
                Kind::Type,
                format!("`count` needs a collection, not a {}", kind_name(other)),
            )),
        });
        vm.native("first", 1, false, |_, a| match &a[0] {
            Value::Nil => Ok(Value::Nil),
            Value::List(l) => Ok(l.0.first().cloned().unwrap_or(Value::Nil)),
            Value::Vec(x) => Ok(x.0.first().cloned().unwrap_or(Value::Nil)),
            other => Err(fault(
                Kind::Type,
                format!("`first` needs a sequence, not a {}", kind_name(other)),
            )),
        });
        // Always a list, whatever it was given: `rest` is the sequence
        // operation, and a list is what a sequence prints as.
        vm.native("rest", 1, false, |_, a| {
            let items = match &a[0] {
                Value::Nil => Vec::new(),
                Value::List(l) => l.0.iter().skip(1).cloned().collect(),
                Value::Vec(x) => x.0.iter().skip(1).cloned().collect(),
                other => {
                    return Err(fault(
                        Kind::Type,
                        format!("`rest` needs a sequence, not a {}", kind_name(other)),
                    ))
                }
            };
            Ok(Value::List(Rc::new(ListObj(items))))
        });
        // Strict where `get` is forgiving: an index off the end is a mistake
        // with a position, not a `nil` that turns up somewhere else later.
        vm.native("nth", 2, false, |_, a| {
            let items = seq_items(&a[0], "nth")?;
            let i = index(&a[1], "nth")?;
            items
                .get(i)
                .cloned()
                .ok_or_else(|| fault(Kind::Type, format!("`nth` index {i} of {}", items.len())))
        });
        vm.native("get", 2, true, |_, a| {
            let missing = a.get(2).cloned().unwrap_or(Value::Nil);
            Ok(match &a[0] {
                Value::Nil => missing,
                Value::Map(m) => {
                    m.0.iter()
                        .find(|(k, _)| equal(k, &a[1]))
                        .map(|(_, v)| v.clone())
                        .unwrap_or(missing)
                }
                Value::List(l) => nth_or(&l.0, &a[1], missing),
                Value::Vec(x) => nth_or(&x.0, &a[1], missing),
                other => {
                    return Err(fault(
                        Kind::Type,
                        format!("`get` needs a collection, not a {}", kind_name(other)),
                    ))
                }
            })
        });
        vm.native("contains?", 2, false, |_, a| {
            Ok(Value::Bool(match &a[0] {
                Value::Nil => false,
                Value::Map(m) => m.0.iter().any(|(k, _)| equal(k, &a[1])),
                // A key, not a value — Clojure's rule, and the one that
                // surprises people who expect it to search a vector.
                Value::List(l) => in_range(l.0.len(), &a[1]),
                Value::Vec(x) => in_range(x.0.len(), &a[1]),
                other => {
                    return Err(fault(
                        Kind::Type,
                        format!("`contains?` needs a collection, not a {}", kind_name(other)),
                    ))
                }
            }))
        });
        vm.native("empty?", 1, false, |_, a| {
            Ok(Value::Bool(match &a[0] {
                Value::Nil => true,
                Value::List(l) => l.0.is_empty(),
                Value::Vec(x) => x.0.is_empty(),
                Value::Map(m) => m.0.is_empty(),
                other => {
                    return Err(fault(
                        Kind::Type,
                        format!("`empty?` needs a collection, not a {}", kind_name(other)),
                    ))
                }
            }))
        });
        // Where the copy-on-write lives (ADR-041 part 1). A list grows at the
        // front and a vector at the back, as in Clojure — `conj` adds where the
        // representation is cheap, and says so by doing it.
        vm.native("conj", 1, true, |_, a| {
            let rest = &a[1..];
            Ok(match &a[0] {
                Value::Nil => Value::List(Rc::new(ListObj(rest.iter().rev().cloned().collect()))),
                Value::List(l) => {
                    let mut out = l.clone();
                    let items = &mut Rc::make_mut(&mut out).0;
                    for v in rest {
                        items.insert(0, v.clone());
                    }
                    Value::List(out)
                }
                Value::Vec(x) => {
                    let mut out = x.clone();
                    Rc::make_mut(&mut out).0.extend(rest.iter().cloned());
                    Value::Vec(out)
                }
                Value::Map(m) => {
                    let mut out = m.clone();
                    for v in rest {
                        let pair = seq_items(v, "conj")?;
                        if pair.len() != 2 {
                            return Err(fault(
                                Kind::Type,
                                "`conj` on a map needs a key/value pair".to_string(),
                            ));
                        }
                        put(
                            &mut Rc::make_mut(&mut out).0,
                            pair[0].clone(),
                            pair[1].clone(),
                        );
                    }
                    Value::Map(out)
                }
                other => {
                    return Err(fault(
                        Kind::Type,
                        format!("`conj` needs a collection, not a {}", kind_name(other)),
                    ))
                }
            })
        });
        vm.native("assoc", 3, true, |_, a| {
            if !a[1..].len().is_multiple_of(2) {
                return Err(fault(
                    Kind::Arity,
                    "`assoc` needs a value for every key".to_string(),
                ));
            }
            Ok(match &a[0] {
                Value::Nil | Value::Map(_) => {
                    let mut out = match &a[0] {
                        Value::Map(m) => m.clone(),
                        _ => Rc::new(MapObj(Vec::new())),
                    };
                    let pairs = &mut Rc::make_mut(&mut out).0;
                    for p in a[1..].chunks(2) {
                        put(pairs, p[0].clone(), p[1].clone());
                    }
                    Value::Map(out)
                }
                Value::Vec(x) => {
                    let mut out = x.clone();
                    let items = &mut Rc::make_mut(&mut out).0;
                    for p in a[1..].chunks(2) {
                        let i = index(&p[0], "assoc")?;
                        if i >= items.len() {
                            return Err(fault(
                                Kind::Type,
                                format!("`assoc` index {i} of {}", items.len()),
                            ));
                        }
                        items[i] = p[1].clone();
                    }
                    Value::Vec(out)
                }
                other => {
                    return Err(fault(
                        Kind::Type,
                        format!("`assoc` needs a map or vector, not a {}", kind_name(other)),
                    ))
                }
            })
        });
        vm.native("dissoc", 1, true, |_, a| {
            Ok(match &a[0] {
                Value::Nil => Value::Nil,
                Value::Map(m) => {
                    let mut out = m.clone();
                    Rc::make_mut(&mut out)
                        .0
                        .retain(|(k, _)| !a[1..].iter().any(|d| equal(k, d)));
                    Value::Map(out)
                }
                other => {
                    return Err(fault(
                        Kind::Type,
                        format!("`dissoc` needs a map, not a {}", kind_name(other)),
                    ))
                }
            })
        });
        vm.native("keys", 1, false, |_, a| map_part(&a[0], "keys", true));
        vm.native("vals", 1, false, |_, a| map_part(&a[0], "vals", false));

        // --- cells (ADR-020, ADR-025) ----------------------------------------
        // The mutable layer. `set-cell!` is a core form and until now there was
        // no way to make the thing it writes to.
        vm.native("cell", 1, false, |vm, a| {
            Ok(Value::Cell(vm.new_cell(a[0].clone())))
        });
        vm.native("cell-get", 1, false, |vm, a| match &a[0] {
            Value::Cell(id) => vm
                .cell(*id)
                .cloned()
                .ok_or_else(|| fault(Kind::Internal, "cell is no longer live".to_string())),
            other => Err(fault(
                Kind::Type,
                format!("`cell-get` needs a cell, not a {}", kind_name(other)),
            )),
        });

        // --- strings and bytes (ADR-018, ADR-041 part 5) ----------------------
        vm.native("str", 0, true, |vm, a| {
            let mut out = String::new();
            for v in a {
                out.push_str(&printer::display(v, &vm.interner));
            }
            Ok(Value::Str(Rc::new(StrObj(out))))
        });
        vm.native("str-len", 1, false, |_, a| {
            Ok(Value::Int(string(&a[0], "str-len")?.len() as i64))
        });
        // Byte indices, because ADR-018 promises no O(1) character indexing.
        // A cut that lands inside a character is an error rather than a panic
        // in the renderer downstream.
        vm.native("str-slice", 3, false, |_, a| {
            let s = string(&a[0], "str-slice")?;
            let (from, to) = (index(&a[1], "str-slice")?, index(&a[2], "str-slice")?);
            if from > to || to > s.len() {
                return Err(fault(
                    Kind::Type,
                    format!("`str-slice` {from}..{to} of {} bytes", s.len()),
                ));
            }
            if !s.is_char_boundary(from) || !s.is_char_boundary(to) {
                return Err(fault(
                    Kind::Type,
                    format!("`str-slice` {from}..{to} splits a character"),
                ));
            }
            Ok(Value::Str(Rc::new(StrObj(s[from..to].to_string()))))
        });
        // ADR-046. The language had no string-to-number conversion at all, and
        // the only path that worked was `json/decode` — an *optional host
        // adapter*, so `--no-default-features` removed the ability to read a
        // number out of text. ADR-013 says features gate host capability and
        // never language semantics; this is that claim being made true again.
        //
        // It is `reader::parse_number` and not a second implementation, so a
        // literal and a parsed string cannot disagree about what a number is.
        // `nil` for a string that is not numeric-looking; a fault for one that
        // is and still is not a number, which is where the reader's diagnostics
        // are worth more than a second `nil` the caller has to guess about.
        vm.native(
            "parse-number",
            1,
            false,
            |_, a| match crate::reader::parse_number(string(&a[0], "parse-number")?) {
                None => Ok(Value::Nil),
                Some(Ok(v)) => Ok(v),
                Some(Err(msg)) => Err(fault(Kind::Type, msg)),
            },
        );
        // Scalar values as integers, since ADR-025 has no character type.
        vm.native("str-scalars", 1, false, |_, a| {
            let s = string(&a[0], "str-scalars")?;
            let items = s.chars().map(|c| Value::Int(c as i64)).collect();
            Ok(Value::Vec(Rc::new(VecObj(items))))
        });
        vm.native("scalars-str", 1, false, |_, a| {
            let items = seq_items(&a[0], "scalars-str")?;
            let mut out = String::new();
            for v in &items {
                let n = index(v, "scalars-str")? as u32;
                match char::from_u32(n) {
                    Some(c) => out.push(c),
                    None => {
                        return Err(fault(
                            Kind::Type,
                            format!("{n} is not a Unicode scalar value"),
                        ))
                    }
                }
            }
            Ok(Value::Str(Rc::new(StrObj(out))))
        });
        vm.native("str-bytes", 1, false, |_, a| {
            let s = string(&a[0], "str-bytes")?;
            Ok(Value::Bytes(Rc::new(BytesObj(s.as_bytes().to_vec()))))
        });
        vm.native("bytes-str", 1, false, |_, a| match &a[0] {
            Value::Bytes(b) => match std::str::from_utf8(&b.0) {
                Ok(s) => Ok(Value::Str(Rc::new(StrObj(s.to_string())))),
                // ADR-018 asks for defined behaviour here rather than a
                // replacement character nobody notices.
                Err(e) => Err(fault(
                    Kind::Type,
                    format!("`bytes-str`: not valid UTF-8 at byte {}", e.valid_up_to()),
                )),
            },
            other => Err(fault(
                Kind::Type,
                format!("`bytes-str` needs bytes, not a {}", kind_name(other)),
            )),
        });
        vm.native("bytes-len", 1, false, |_, a| match &a[0] {
            Value::Bytes(b) => Ok(Value::Int(b.0.len() as i64)),
            other => Err(fault(
                Kind::Type,
                format!("`bytes-len` needs bytes, not a {}", kind_name(other)),
            )),
        });

        vm.native("println", 0, true, |vm, a| {
            let line: Vec<String> = a
                .iter()
                .map(|v| printer::display(v, &vm.interner))
                .collect();
            vm.emit(&line.join(" "));
            vm.emit("\n");
            Ok(Value::Nil)
        });
        // ADR-040: the explicit half of capture avoidance. Auto-gensym
        // (`x#`) covers the common case in a template; this is for a name
        // a macro has to compute.
        vm.native("gensym", 0, true, |vm, a| {
            let prefix = match a.first() {
                None => "G".to_string(),
                Some(Value::Str(s)) => s.0.clone(),
                Some(Value::Sym(s)) => vm.interner.name(s.0).to_string(),
                Some(other) => {
                    return Err(fault(
                        Kind::Type,
                        format!(
                            "`gensym` needs a string or symbol prefix, not a {}",
                            crate::value::kind_name(other)
                        ),
                    ))
                }
            };
            let name = vm.gensym_name(&prefix);
            Ok(vm.interner.sym(&name))
        });
        vm.native("list", 0, true, |_, a| {
            Ok(Value::List(Rc::new(ListObj(a.to_vec()))))
        });
        // ADR-035 lowers `[a b]` to a call, so these two are what makes a
        // collection literal mean anything. The representation itself is
        // still Q6's, and `hash-map` keeps duplicate keys because Q20 has
        // not said what to do with them.
        // Quasiquote's `~@` lowers to this (ADR-040). Deliberately minimal:
        // it takes the sequential collections that exist and yields a list.
        // Q6 owns the general one at milestone 6.
        vm.native("concat", 0, true, |_, a| {
            let mut out = Vec::new();
            for v in a {
                match v {
                    Value::List(l) => out.extend(l.0.iter().cloned()),
                    Value::Vec(x) => out.extend(x.0.iter().cloned()),
                    Value::Nil => {}
                    other => {
                        return Err(fault(
                            Kind::Type,
                            format!(
                                "`concat` needs lists or vectors, not a {}",
                                crate::value::kind_name(other)
                            ),
                        ))
                    }
                }
            }
            Ok(Value::List(Rc::new(ListObj(out))))
        });
        // The other half of a vector template's lowering: build a list,
        // then convert. Also Q6's, eventually.
        vm.native("vec", 1, false, |_, a| match &a[0] {
            Value::List(l) => Ok(Value::Vec(Rc::new(VecObj(l.0.clone())))),
            Value::Vec(x) => Ok(Value::Vec(x.clone())),
            other => Err(fault(
                Kind::Type,
                format!(
                    "`vec` needs a list or vector, not a {}",
                    crate::value::kind_name(other)
                ),
            )),
        });
        vm.native("vector", 0, true, |_, a| {
            Ok(Value::Vec(Rc::new(VecObj(a.to_vec()))))
        });
        vm.native("hash-map", 0, true, |_, a| {
            if !a.len().is_multiple_of(2) {
                return Err(fault(Kind::Arity, "`hash-map` needs a value for every key"));
            }
            // Last write wins, so a map never holds two equal keys (ADR-041
            // part 4). That this was not already true is the first thing the
            // in-language suite caught.
            let mut pairs = Vec::new();
            for p in a.chunks(2) {
                put(&mut pairs, p[0].clone(), p[1].clone());
            }
            Ok(Value::Map(Rc::new(MapObj(pairs))))
        });
    }

    // --- collection helpers ---------------------------------------------------

    fn seq_items(v: &Value, op: &str) -> Result<Vec<Value>, Fault> {
        match v {
            Value::Nil => Ok(Vec::new()),
            Value::List(l) => Ok(l.0.clone()),
            Value::Vec(x) => Ok(x.0.clone()),
            other => Err(fault(
                Kind::Type,
                format!("`{op}` needs a sequence, not a {}", kind_name(other)),
            )),
        }
    }

    fn string<'a>(v: &'a Value, op: &str) -> Result<&'a str, Fault> {
        match v {
            Value::Str(s) => Ok(&s.0),
            other => Err(fault(
                Kind::Type,
                format!("`{op}` needs a string, not a {}", kind_name(other)),
            )),
        }
    }

    fn index(v: &Value, op: &str) -> Result<usize, Fault> {
        match v {
            Value::Int(i) if *i >= 0 => Ok(*i as usize),
            Value::Int(i) => Err(fault(
                Kind::Type,
                format!("`{op}` needs a non-negative index, not {i}"),
            )),
            other => Err(fault(
                Kind::Type,
                format!("`{op}` needs an integer index, not a {}", kind_name(other)),
            )),
        }
    }

    fn nth_or(items: &[Value], key: &Value, missing: Value) -> Value {
        match key {
            Value::Int(i) if *i >= 0 => items.get(*i as usize).cloned().unwrap_or(missing),
            _ => missing,
        }
    }

    fn in_range(len: usize, key: &Value) -> bool {
        matches!(key, Value::Int(i) if *i >= 0 && (*i as usize) < len)
    }

    /// Last write wins, and a map never holds two equal keys (ADR-041 part 4).
    fn put(pairs: &mut Vec<(Value, Value)>, k: Value, v: Value) {
        match pairs.iter_mut().find(|(existing, _)| equal(existing, &k)) {
            Some(slot) => slot.1 = v,
            None => pairs.push((k, v)),
        }
    }

    fn map_part(v: &Value, op: &str, want_key: bool) -> Result<Value, Fault> {
        let pairs = match v {
            Value::Nil => Vec::new(),
            Value::Map(m) => m.0.clone(),
            other => {
                return Err(fault(
                    Kind::Type,
                    format!("`{op}` needs a map, not a {}", kind_name(other)),
                ))
            }
        };
        let items = pairs
            .into_iter()
            .map(|(k, val)| if want_key { k } else { val })
            .collect();
        Ok(Value::List(Rc::new(ListObj(items))))
    }

    /// `quot` and `rem`: integer division, and the one place dividing by zero
    /// is an error rather than an infinity (ADR-041 part 3).
    fn int_div(a: &[Value], op: &str) -> Result<Value, Fault> {
        let (x, y) = match (num(&a[0], op)?, num(&a[1], op)?) {
            (Num::Int(x), Num::Int(y)) => (x, y),
            _ => {
                return Err(fault(
                    Kind::Type,
                    format!("`{op}` needs integers; `/` is the float division"),
                ))
            }
        };
        if y == 0 {
            return Err(fault(Kind::DivideByZero, format!("`{op}` by zero")));
        }
        // `checked_*` covers the one overflowing case, i64::MIN / -1.
        let r = if op == "quot" {
            x.checked_div(y)
        } else {
            x.checked_rem(y)
        };
        Ok(Value::Int(r.ok_or_else(|| overflow(op))?))
    }

    fn overflow(op: &str) -> Fault {
        fault(
            Kind::Overflow,
            format!("`{op}` overflowed a 64-bit integer (ADR-037)"),
        )
    }

    /// One arithmetic operand, as the tower sees it (ADR-041 part 3). A float
    /// anywhere in the operands makes the whole operation a float one, which is
    /// the only rule the caller needs.
    #[derive(Clone, Copy)]
    enum Num {
        Int(i64),
        Float(f64),
    }

    fn num(v: &Value, op: &str) -> Result<Num, Fault> {
        match v {
            Value::Int(i) => Ok(Num::Int(*i)),
            Value::Float(f) => Ok(Num::Float(*f)),
            other => Err(fault(
                Kind::Type,
                format!("`{op}` needs a number, not a {}", kind_name(other)),
            )),
        }
    }

    fn as_f64(n: Num) -> f64 {
        match n {
            Num::Int(i) => i as f64,
            Num::Float(f) => f,
        }
    }

    /// Fold the arguments, staying in integers until a float appears.
    ///
    /// The two halves fail differently on purpose: an integer that leaves the
    /// range throws (ADR-037, a wrong answer with no diagnostic is the thing to
    /// prevent), and a float that leaves it becomes `##Inf`, which is IEEE's
    /// own out-of-range value and prints as itself (ADR-041 part 3).
    fn fold(
        args: &[Value],
        init: i64,
        int_op: fn(i64, i64) -> Option<i64>,
        float_op: fn(f64, f64) -> f64,
        name: &str,
    ) -> Result<Value, Fault> {
        let mut acc = Num::Int(init);
        for v in args {
            let n = num(v, name)?;
            acc = match (acc, n) {
                (Num::Int(a), Num::Int(b)) => Num::Int(int_op(a, b).ok_or_else(|| overflow(name))?),
                _ => Num::Float(float_op(as_f64(acc), as_f64(n))),
            };
        }
        Ok(number(acc))
    }

    fn number(n: Num) -> Value {
        match n {
            Num::Int(i) => Value::Int(i),
            Num::Float(f) => Value::Float(f),
        }
    }

    /// Compare two numbers across the tower. `==` and the ordering primitives
    /// share this, which is why `(< 1 1.5)` and `(== 1 1.0)` cannot disagree
    /// about what a mixed comparison means.
    fn compare(a: &Value, b: &Value, op: &str) -> Result<std::cmp::Ordering, Fault> {
        let (x, y) = (num(a, op)?, num(b, op)?);
        match (x, y) {
            (Num::Int(a), Num::Int(b)) => Ok(a.cmp(&b)),
            _ => as_f64(x)
                .partial_cmp(&as_f64(y))
                // `##NaN` is unordered against everything, itself included.
                // Refusing beats inventing an answer that would make `<` and
                // `>` both false and `=` false as well, with no diagnostic.
                .ok_or_else(|| {
                    fault(
                        Kind::Type,
                        format!("`{op}` on ##NaN, which is unordered (ADR-041)"),
                    )
                }),
        }
    }
}

// ---------------------------------------------------------------------------

/// The host boundary (ADR-016): the only place this system touches something
/// outside itself.
///
/// Language code never holds a Rust object. It holds a generational `HandleId`
/// into the table the VM owns, which is the one representation of a resource
/// that survives a snapshot (ADR-029) — a raw pointer cannot be serialized, an
/// index can. There is no `HostCall` opcode: an io primitive is an ordinary
/// global reached through the ordinary `Call`, which is everything ADR-038
/// already delivered (ADR-042 part 3).
pub mod host {
    use crate::value::{kind_name, BytesObj, HandleId, Value};
    use crate::vm::{Fault, Kind, Vm};
    use std::io::Read;
    // Only a file is written through a `Write`. `io/stdout` is the buffered
    // host and goes through `Vm::emit`, so without `fs` nothing in this module
    // writes to the outside world at all — which is the subtraction working.
    #[cfg(any(feature = "fs", feature = "tcp"))]
    use std::io::Write;
    use std::rc::Rc;

    /// A live host resource. Cutting a variant is what ADR-013 means by a seam
    /// for subtraction: the primitive that constructs it goes with it, and
    /// nothing in the VM changes.
    pub enum Host {
        /// ADR-013's subtraction harness, and the first place it is real: build
        /// without `fs` and this variant, the primitive that constructs it, and
        /// the three arms that read it all go. Nothing else in the VM changes,
        /// which is the claim the feature exists to keep testable.
        #[cfg(feature = "fs")]
        File(std::fs::File),
        Stdin,
        /// The buffered in-memory host, *not* a file descriptor. ADR-029 needs
        /// emitted effects to be part of the serialization comparison rather
        /// than escaping it, so a write here goes where `println` goes.
        Stdout,
        /// ADR-045 part 5: a socket is a handle like a file is, so it reads and
        /// writes through the same primitives and refuses a snapshot through
        /// the same check. ADR-043 declares only the standard streams
        /// reconstructible, so a live connection makes `capture` fail with no
        /// new code at all.
        #[cfg(feature = "tcp")]
        Tcp(std::net::TcpStream),
        #[cfg(feature = "tcp")]
        Listener(std::net::TcpListener),
    }

    /// The closed `:kind` vocabulary for `:type :io-error` (ADR-042 part 1).
    /// The three network kinds the design conversation proposed are absent on
    /// purpose: nothing here can raise one, and they arrive with the adapter
    /// that can.
    #[derive(Clone, Copy, Debug)]
    pub enum IoKind {
        NotFound,
        PermissionDenied,
        Closed,
        InvalidData,
        Interrupted,
        /// ADR-045 part 2. ADR-042 named these three and deferred them to the
        /// entry that adds the subsystem raising them, on the grounds that a
        /// kind nobody can raise is a guess with a colon in front of it. TCP
        /// is that subsystem; a file can produce none of them.
        Timeout,
        WouldBlock,
        ConnectionReset,
        /// Documented as *do not dispatch on this, read the message*. It exists
        /// because `std::io::ErrorKind` is `#[non_exhaustive]` and the set of
        /// things an operating system can refuse is not ours to close.
        Other,
    }

    impl IoKind {
        /// In discriminant order, which is what `kind as usize` indexes.
        pub const ALL: [IoKind; 9] = [
            IoKind::NotFound,
            IoKind::PermissionDenied,
            IoKind::Closed,
            IoKind::InvalidData,
            IoKind::Interrupted,
            IoKind::Timeout,
            IoKind::WouldBlock,
            IoKind::ConnectionReset,
            IoKind::Other,
        ];

        pub fn name(self) -> &'static str {
            match self {
                IoKind::NotFound => "not-found",
                IoKind::PermissionDenied => "permission-denied",
                IoKind::Closed => "closed",
                IoKind::InvalidData => "invalid-data",
                IoKind::Interrupted => "interrupted",
                IoKind::Timeout => "timeout",
                IoKind::WouldBlock => "would-block",
                IoKind::ConnectionReset => "connection-reset",
                IoKind::Other => "other",
            }
        }
    }

    /// The primitive that failed. Closed for the same reason the kinds are, and
    /// for one more: `:operation` is a keyword, and interning one at a raise
    /// site would need `&mut Vm` where building a fault value has only `&Vm`.
    #[derive(Clone, Copy, Debug)]
    pub enum IoOp {
        Open,
        Close,
        Read,
        Write,
        /// The socket operations (ADR-045). `:connect` and `:accept` are the
        /// two a `:timeout` or a `:connection-reset` actually comes out of, so
        /// naming them is what makes those kinds dispatchable.
        Connect,
        Listen,
        Accept,
        Encode,
        Decode,
    }

    impl IoOp {
        pub const ALL: [IoOp; 9] = [
            IoOp::Open,
            IoOp::Close,
            IoOp::Read,
            IoOp::Write,
            IoOp::Connect,
            IoOp::Listen,
            IoOp::Accept,
            IoOp::Encode,
            IoOp::Decode,
        ];

        pub fn name(self) -> &'static str {
            match self {
                IoOp::Open => "open",
                IoOp::Close => "close",
                IoOp::Read => "read",
                IoOp::Write => "write",
                IoOp::Connect => "connect",
                IoOp::Listen => "listen",
                IoOp::Accept => "accept",
                IoOp::Encode => "encode",
                IoOp::Decode => "decode",
            }
        }
    }

    /// ADR-042 part 2: the message is ours, and the raw code is dropped.
    /// `std::io::Error`'s own `Display` carries an errno and platform wording —
    /// "No such file or directory (os error 2)" on one host, other words on
    /// another — and ADR-039 makes a thrown value printable into a `.out`
    /// golden, so forwarding it would put the machine in the oracle.
    fn classify(e: &std::io::Error) -> (IoKind, &'static str) {
        use std::io::ErrorKind as E;
        match e.kind() {
            E::NotFound => (IoKind::NotFound, "no such file"),
            E::PermissionDenied => (IoKind::PermissionDenied, "permission denied"),
            E::InvalidData => (IoKind::InvalidData, "not valid data"),
            E::Interrupted => (IoKind::Interrupted, "interrupted"),
            E::TimedOut => (IoKind::Timeout, "the operation timed out"),
            E::WouldBlock => (IoKind::WouldBlock, "the operation would block"),
            E::ConnectionReset | E::ConnectionAborted | E::BrokenPipe => {
                (IoKind::ConnectionReset, "the connection was reset")
            }
            // `ConnectionRefused` is deliberately not `:connection-reset`:
            // nothing was ever connected, so a program retrying a reset would
            // retry the wrong thing.
            _ => (IoKind::Other, "the host refused the operation"),
        }
    }

    pub(crate) fn host_failed(op: IoOp, path: Option<String>, e: &std::io::Error) -> Fault {
        let (kind, msg) = classify(e);
        Fault::Io {
            op,
            path,
            kind,
            msg: msg.to_string(),
        }
    }

    pub(crate) fn io_fault(op: IoOp, kind: IoKind, msg: impl Into<String>) -> Fault {
        Fault::Io {
            op,
            path: None,
            kind,
            msg: msg.into(),
        }
    }

    /// A closed *or* stale handle. Both are `:closed` to a program: the id does
    /// not name a live resource, and which of the two ways it fails to is a
    /// distinction only `io/close` draws (ADR-042 part 4).
    pub(crate) fn not_live(op: IoOp) -> Fault {
        io_fault(
            op,
            IoKind::Closed,
            "the handle does not name an open resource",
        )
    }

    /// A misuse is a `:vm-error :type`, not an `:io-error`. Passing a string
    /// where a handle belongs is the same class of mistake as `(+ 1 "x")`, and
    /// nothing about the host was involved.
    pub(crate) fn misuse(msg: impl Into<String>) -> Fault {
        Fault::Vm {
            kind: Kind::Type,
            msg: msg.into(),
        }
    }

    pub(crate) fn handle_arg(v: &Value, op: &str) -> Result<HandleId, Fault> {
        match v {
            Value::Handle(h) => Ok(*h),
            other => Err(misuse(format!(
                "`{op}` needs a handle, not a {}",
                kind_name(other)
            ))),
        }
    }

    #[cfg(any(feature = "fs", feature = "tcp"))]
    pub(crate) fn string_arg<'a>(v: &'a Value, op: &str) -> Result<&'a str, Fault> {
        match v {
            Value::Str(s) => Ok(&s.0),
            other => Err(misuse(format!(
                "`{op}` needs a string, not a {}",
                kind_name(other)
            ))),
        }
    }

    #[cfg(feature = "fs")]
    fn install_fs(vm: &mut Vm) {
        vm.native("io/open", 2, false, |vm, a| {
            let path = string_arg(&a[0], "io/open")?.to_string();
            let mode = match &a[1] {
                // Cloned because opening needs `&mut vm` and the name borrows
                // the interner out of it.
                Value::Keyword(k) => vm.interner.name(k.0).to_string(),
                other => {
                    return Err(misuse(format!(
                        "`io/open` needs a mode keyword, not a {}",
                        kind_name(other)
                    )))
                }
            };
            let mut opts = std::fs::OpenOptions::new();
            match mode.as_str() {
                "read" => opts.read(true),
                "write" => opts.write(true).create(true).truncate(true),
                "append" => opts.append(true).create(true),
                other => {
                    return Err(misuse(format!(
                        "`io/open`: mode is :read, :write, or :append, not :{other}"
                    )))
                }
            };
            match opts.open(&path) {
                Ok(f) => Ok(Value::Handle(vm.open_handle(Host::File(f)))),
                Err(e) => Err(host_failed(IoOp::Open, Some(path), &e)),
            }
        });
    }

    /// How many handles a fresh VM of this build already has open, and which
    /// ADR-043 part 5 declares reconstructible: `io/stdin` and `io/stdout`.
    /// A snapshot refuses anything beyond them.
    pub const RECONSTRUCTIBLE: usize = 2;

    pub fn install(vm: &mut Vm) {
        // Not functions. ADR-038 made a primitive an ordinary global, so the
        // two standard streams are *values* in the global table — one handle
        // each, created once, rather than a native that mints a new one per
        // call and leaks a table slot every time it is asked.
        let stdin = vm.open_handle(Host::Stdin);
        let stdout = vm.open_handle(Host::Stdout);
        vm.set_named_global("io/stdin", Value::Handle(stdin));
        vm.set_named_global("io/stdout", Value::Handle(stdout));

        #[cfg(feature = "fs")]
        install_fs(vm);

        vm.native("io/close", 1, false, |vm, a| {
            let id = handle_arg(&a[0], "io/close")?;
            // Dropping the `File` is what closes the descriptor, and it is also
            // what flushes it — which is why there is no `io/flush`.
            if vm.close_handle(id) {
                Ok(Value::Nil)
            } else {
                Err(io_fault(
                    IoOp::Close,
                    IoKind::Closed,
                    "the handle is stale: its slot has been reused",
                ))
            }
        });

        vm.native("io/open?", 1, false, |vm, a| {
            let id = handle_arg(&a[0], "io/open?")?;
            Ok(Value::Bool(vm.host(id).is_some()))
        });

        vm.native("io/read", 2, false, |vm, a| {
            let id = handle_arg(&a[0], "io/read")?;
            let n = match &a[1] {
                Value::Int(i) if *i >= 0 => *i as usize,
                other => {
                    return Err(misuse(format!(
                        "`io/read` needs a non-negative count, not {}",
                        crate::printer::display(other, &vm.interner)
                    )))
                }
            };
            let mut buf = vec![0u8; n];
            let got = match vm.host_mut(id).ok_or_else(|| not_live(IoOp::Read))? {
                #[cfg(feature = "fs")]
                Host::File(f) => f.read(&mut buf),
                #[cfg(feature = "tcp")]
                Host::Tcp(t) => t.read(&mut buf),
                #[cfg(feature = "tcp")]
                Host::Listener(_) => return Err(misuse("`io/read` cannot read from a listener")),
                Host::Stdin => std::io::stdin().read(&mut buf),
                Host::Stdout => return Err(misuse("`io/read` cannot read from `io/stdout`")),
            };
            // A short read is not an error and neither is end of input: empty
            // bytes is the end, which is what lets a read loop terminate on a
            // value rather than on a caught throw.
            match got {
                Ok(got) => {
                    buf.truncate(got);
                    Ok(Value::Bytes(Rc::new(BytesObj(buf))))
                }
                Err(e) => Err(host_failed(IoOp::Read, None, &e)),
            }
        });

        vm.native("io/read-all", 1, false, |vm, a| {
            let id = handle_arg(&a[0], "io/read-all")?;
            let mut buf = Vec::new();
            let got = match vm.host_mut(id).ok_or_else(|| not_live(IoOp::Read))? {
                #[cfg(feature = "fs")]
                Host::File(f) => f.read_to_end(&mut buf),
                #[cfg(feature = "tcp")]
                Host::Tcp(t) => t.read_to_end(&mut buf),
                #[cfg(feature = "tcp")]
                Host::Listener(_) => {
                    return Err(misuse("`io/read-all` cannot read from a listener"))
                }
                Host::Stdin => std::io::stdin().read_to_end(&mut buf),
                Host::Stdout => return Err(misuse("`io/read-all` cannot read from `io/stdout`")),
            };
            match got {
                Ok(_) => Ok(Value::Bytes(Rc::new(BytesObj(buf)))),
                Err(e) => Err(host_failed(IoOp::Read, None, &e)),
            }
        });

        vm.native("io/write", 2, false, |vm, a| {
            let id = handle_arg(&a[0], "io/write")?;
            let bytes = match &a[1] {
                Value::Bytes(b) => b.0.clone(),
                Value::Str(s) => s.0.as_bytes().to_vec(),
                other => {
                    return Err(misuse(format!(
                        "`io/write` needs bytes or a string, not a {}",
                        kind_name(other)
                    )))
                }
            };
            let n = Value::Int(bytes.len() as i64);
            // Stdout is looked up twice on purpose. `vm.emit` needs `&mut vm`
            // and `host_mut` is already holding one, so the buffered case has
            // to be settled before the borrow is taken rather than inside the
            // match that would be the obvious place for it.
            if matches!(vm.host(id), Some(Host::Stdout)) {
                return match String::from_utf8(bytes) {
                    Ok(text) => {
                        vm.emit(&text);
                        Ok(n)
                    }
                    Err(e) => Err(io_fault(
                        IoOp::Write,
                        IoKind::InvalidData,
                        format!(
                            "`io/stdout` takes text, and these bytes are not valid UTF-8 at byte {}",
                            e.utf8_error().valid_up_to()
                        ),
                    )),
                };
            }
            match vm.host_mut(id).ok_or_else(|| not_live(IoOp::Write))? {
                #[cfg(feature = "fs")]
                Host::File(f) => f
                    .write_all(&bytes)
                    .map(|()| n)
                    .map_err(|e| host_failed(IoOp::Write, None, &e)),
                #[cfg(feature = "tcp")]
                Host::Tcp(t) => t
                    .write_all(&bytes)
                    .map(|()| n)
                    .map_err(|e| host_failed(IoOp::Write, None, &e)),
                #[cfg(feature = "tcp")]
                Host::Listener(_) => Err(misuse("`io/write` cannot write to a listener")),
                Host::Stdin => Err(misuse("`io/write` cannot write to `io/stdin`")),
                Host::Stdout => unreachable!("settled above, before the mutable borrow"),
            }
        });
    }
}

// ---------------------------------------------------------------------------

/// Fuel suspension's other half: turning a live `Vm` and `Execution` into
/// something with no `Rc` in it, and back (ADR-029, ADR-043).
///
/// The encoding is object-id based, and sharing is preserved: two `Rc`s at the
/// same address encode to the same id. That is not a size optimization —
/// expanding shared structure into copies is exponential, and `(def b [a a])`
/// four times over is sixteen copies of one vector (ADR-043 part 2).
///
/// v1 does not serialize this to bytes. Once no `Rc` appears inside an `Image`,
/// a `serde` derive is plumbing rather than design, which is what ADR-029
/// claimed and what leaving it out lets this milestone keep as a claim honestly.
pub mod image {
    use crate::bytecode::{Chunk, Slot};
    use crate::error::SpanOrigin;
    use crate::value::{
        BufferId, BytesObj, CellId, Closure, HandleId, Interner, KwId, ListObj, MapObj, NativeId,
        StrObj, SymId, Value, VecObj,
    };
    use crate::vm::{CellEntry, Execution, Frame, HandleEntry, Handler, Pending, Unwind, Vm};
    use std::collections::HashMap;
    use std::rc::Rc;

    /// Why a snapshot was refused. Both are ADR-029's promises, as values
    /// rather than as prose.
    #[derive(Debug, PartialEq)]
    pub enum SnapshotError {
        /// ADR-029: adapter checkpointing is a later opt-in, so a live resource
        /// is a refusal and not a best effort. The count excludes the standard
        /// streams, which ADR-043 part 5 declares reconstructible.
        SnapshotHasLiveHandles(usize),
        /// ADR-029: same-build, same-code only. The `Image` carries a
        /// fingerprint rather than the chunk, so this is the check that makes
        /// "same build" something other than an assumption.
        ChunkMismatch,
    }

    /// A `Value` with the pointers taken out. Every variant is `Copy` and
    /// self-contained; anything that lived behind an `Rc` became an `Obj`.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub enum Ref {
        Nil,
        Bool(bool),
        /// Bits, not an `f64`. ADR-032 exists because `##NaN` and `-0.0` have to
        /// survive a round trip exactly, and bit patterns are the only spelling
        /// that cannot quietly normalise one into the other.
        Float(u64),
        Int(i64),
        Sym(u32),
        Keyword(u32),
        Cell(u32, u32),
        Handle(u32, u32),
        Buffer(u32, u32),
        /// Index into `Image::objects`.
        Obj(u32),
    }

    /// Everything that lived behind an `Rc`, flattened. Children are `Ref`s, so
    /// an `Obj` is one level deep no matter how deep the value was.
    #[derive(Debug)]
    pub enum Obj {
        Str(String),
        Bytes(Vec<u8>),
        List(Vec<Ref>),
        Vec(Vec<Ref>),
        Map(Vec<(Ref, Ref)>),
        Fn { proto: u32, captures: Vec<Ref> },
        Native(u32),
    }

    #[derive(Debug)]
    pub struct FrameDto {
        proto: u32,
        pc: usize,
        base: usize,
        dst: usize,
        ret_len: usize,
        /// An object id, so a closure shared between a frame and a slot stays
        /// one closure across the round trip.
        closure: u32,
    }

    #[derive(Debug)]
    pub struct HandlerDto {
        frame: usize,
        target: usize,
        err: Option<Slot>,
    }

    /// An unwind parked mid-cleanup. It has to travel: a snapshot taken while a
    /// `finally` runs and a resume that forgot the parked error would drop the
    /// original failure and report success (ADR-028 invariant 3).
    #[derive(Debug)]
    pub struct PendingDto {
        depth: usize,
        value: Ref,
        origin: SpanOrigin,
        suppressed: Vec<Ref>,
    }

    /// One `Vm` plus one suspended `Execution`, and nothing else — ADR-029's
    /// promise is that anything not in one of the two is out of scope by
    /// construction rather than by oversight.
    #[derive(Debug)]
    pub struct Image {
        fingerprint: u64,
        names: Vec<String>,
        objects: Vec<Obj>,
        globals: Vec<Option<(u32, u32)>>,
        cells: Vec<(u32, Ref)>,
        /// Generations only. The resources themselves are gone by construction:
        /// a capture refuses while any non-reconstructible handle is open, so
        /// every slot here is either a standard stream or closed. The
        /// generations still matter, because a later `io/open` reuses an index
        /// and the id it hands out has to differ from the retired one.
        handle_generations: Vec<u32>,
        free_handles: Vec<u32>,
        gensym: u64,
        out: String,
        frames: Vec<FrameDto>,
        slots: Vec<Ref>,
        handlers: Vec<HandlerDto>,
        pending: Vec<PendingDto>,
        fuel: u64,
    }

    impl Image {
        /// How many distinct heap objects the encoding holds. Exposed because
        /// sharing is invisible from inside the language — `=` is structural
        /// and there is no `identical?` — so this number is the only way a test
        /// can tell a sharing encoder from a copying one (ADR-043 part 2).
        pub fn object_count(&self) -> usize {
            self.objects.len()
        }
    }

    /// Same-build, same-code. Hashing the `Debug` rendering rather than deriving
    /// `Hash` over the tree: `Proto::consts` holds `Value`s, floats included, so
    /// a derive would need a hand-written `Hash` for a numeric tower whose
    /// equality is deliberately not Rust's (`TRAPS.md`). A snapshot is not a hot
    /// path, and one allocation buys total coverage.
    pub fn fingerprint(chunk: &Chunk) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        format!("{chunk:?}").hash(&mut h);
        h.finish()
    }

    // --- capture ------------------------------------------------------------

    struct Encoder {
        objects: Vec<Obj>,
        /// `Rc` address to object id. Encoding therefore depends on addresses,
        /// which are stable within a run and meaningless across one — so no two
        /// `Image`s may ever be compared. The round-trip property compares
        /// transcripts, which is the only comparison this design supports.
        seen: HashMap<usize, u32>,
    }

    impl Encoder {
        /// Children are encoded before their parent is pushed, so an object's
        /// id is always higher than every id it names. That makes the table
        /// topologically ordered and the decoder a single forward loop with no
        /// forward reference to resolve — see erratum E-14.
        fn obj(&mut self, addr: usize, build: impl FnOnce(&mut Encoder) -> Obj) -> Ref {
            if let Some(&id) = self.seen.get(&addr) {
                return Ref::Obj(id);
            }
            let o = build(self);
            let id = self.objects.len() as u32;
            self.objects.push(o);
            self.seen.insert(addr, id);
            Ref::Obj(id)
        }

        fn each(&mut self, vs: &[Value]) -> Vec<Ref> {
            vs.iter().map(|v| self.value(v)).collect()
        }

        fn value(&mut self, v: &Value) -> Ref {
            fn addr<T>(r: &Rc<T>) -> usize {
                Rc::as_ptr(r) as *const u8 as usize
            }
            match v {
                Value::Nil => Ref::Nil,
                Value::Bool(b) => Ref::Bool(*b),
                Value::Int(i) => Ref::Int(*i),
                Value::Float(f) => Ref::Float(f.to_bits()),
                Value::Sym(s) => Ref::Sym(s.0),
                Value::Keyword(k) => Ref::Keyword(k.0),
                Value::Cell(c) => Ref::Cell(c.0, c.1),
                Value::Handle(h) => Ref::Handle(h.0, h.1),
                Value::Buffer(b) => Ref::Buffer(b.0, b.1),
                Value::Str(s) => self.obj(addr(s), |_| Obj::Str(s.0.clone())),
                Value::Bytes(b) => self.obj(addr(b), |_| Obj::Bytes(b.0.clone())),
                Value::List(l) => self.obj(addr(l), |e| Obj::List(e.each(&l.0))),
                Value::Vec(x) => self.obj(addr(x), |e| Obj::Vec(e.each(&x.0))),
                Value::Map(m) => self.obj(addr(m), |e| {
                    Obj::Map(m.0.iter().map(|(k, v)| (e.value(k), e.value(v))).collect())
                }),
                Value::Fn(f) => self.obj(addr(f), |e| match &**f {
                    Closure::Fn { proto, captures } => Obj::Fn {
                        proto: *proto,
                        captures: e.each(captures),
                    },
                    Closure::Native(id) => Obj::Native(id.0),
                }),
            }
        }
    }

    /// Take an `Image` of a suspended run.
    ///
    /// Refused while a non-reconstructible handle is open (ADR-029). The two
    /// standard streams do not count: `host::install` recreates them at the
    /// same ids in any VM of this build, so restoring them is restoring rather
    /// than inventing (ADR-043 part 5). A file is not reconstructible and never
    /// becomes so by that argument.
    pub fn capture(vm: &Vm, ex: &Execution, chunk: &Chunk) -> Result<Image, SnapshotError> {
        let live = vm.open_handles();
        if live > crate::host::RECONSTRUCTIBLE {
            return Err(SnapshotError::SnapshotHasLiveHandles(
                live - crate::host::RECONSTRUCTIBLE,
            ));
        }
        let mut e = Encoder {
            objects: Vec::new(),
            seen: HashMap::new(),
        };
        let cells = vm
            .cells
            .iter()
            .map(|c| (c.generation, e.value(&c.value)))
            .collect();
        let slots = e.each(&ex.slots);
        let frames = ex
            .frames
            .iter()
            .map(|f| FrameDto {
                proto: f.proto,
                pc: f.pc,
                base: f.base,
                dst: f.dst,
                ret_len: f.ret_len,
                closure: match e.value(&Value::Fn(f.closure.clone())) {
                    Ref::Obj(id) => id,
                    _ => unreachable!("a closure always encodes as an object"),
                },
            })
            .collect();
        let pending = ex
            .pending
            .iter()
            .map(|p| PendingDto {
                depth: p.depth,
                value: e.value(&p.unwind.value),
                origin: p.unwind.origin,
                suppressed: e.each(&p.unwind.suppressed),
            })
            .collect();
        Ok(Image {
            fingerprint: fingerprint(chunk),
            names: vm.interner.names().to_vec(),
            globals: vm.globals.iter().map(|g| g.map(|c| (c.0, c.1))).collect(),
            cells,
            handle_generations: vm.handles.iter().map(|h| h.generation).collect(),
            free_handles: vm.free_handles.clone(),
            gensym: vm.gensym,
            out: vm.out.clone(),
            frames,
            slots,
            handlers: ex
                .handlers
                .iter()
                .map(|h| HandlerDto {
                    frame: h.frame,
                    target: h.target,
                    err: h.err,
                })
                .collect(),
            pending,
            fuel: ex.fuel,
            objects: e.objects,
        })
    }

    // --- restore ------------------------------------------------------------

    /// Rebuild a `Vm` and its suspended `Execution` from an `Image`.
    ///
    /// The chunk comes from the caller, not the image: ADR-029 says *code
    /// identity*, and same-build means whoever resumes already has the code.
    /// The fingerprint is what makes that a check rather than an assumption.
    pub fn restore(img: &Image, chunk: &Chunk) -> Result<(Vm, Execution), SnapshotError> {
        if img.fingerprint != fingerprint(chunk) {
            return Err(SnapshotError::ChunkMismatch);
        }
        // A fresh VM of this build, so the natives, the interned fault
        // keywords, and the two standard streams are back in place before
        // anything else is written over them. `install` is deterministic, which
        // is what makes the ids below line up.
        let mut vm = Vm::new();

        // One forward pass. An object's children were encoded first, so every
        // id it names is already built — see erratum E-14.
        let mut objects: Vec<Value> = Vec::with_capacity(img.objects.len());
        for o in &img.objects {
            let get = |r: &Ref| deref(r, &objects);
            let v = match o {
                Obj::Str(s) => Value::Str(Rc::new(StrObj(s.clone()))),
                Obj::Bytes(b) => Value::Bytes(Rc::new(BytesObj(b.clone()))),
                Obj::List(xs) => Value::List(Rc::new(ListObj(xs.iter().map(get).collect()))),
                Obj::Vec(xs) => Value::Vec(Rc::new(VecObj(xs.iter().map(get).collect()))),
                Obj::Map(kvs) => Value::Map(Rc::new(MapObj(
                    kvs.iter().map(|(k, v)| (get(k), get(v))).collect(),
                ))),
                Obj::Fn { proto, captures } => Value::Fn(Rc::new(Closure::Fn {
                    proto: *proto,
                    captures: captures.iter().map(get).collect(),
                })),
                Obj::Native(id) => Value::Fn(Rc::new(Closure::Native(NativeId(*id)))),
            };
            objects.push(v);
        }
        let get = |r: &Ref| deref(r, &objects);

        vm.interner = Interner::restore(img.names.clone());
        vm.globals = img
            .globals
            .iter()
            .map(|g| g.map(|(i, gen)| CellId(i, gen)))
            .collect();
        vm.cells = img
            .cells
            .iter()
            .map(|(gen, r)| CellEntry {
                generation: *gen,
                value: get(r),
            })
            .collect();
        // The reconstructible prefix keeps the resources `install` just made;
        // every slot above it is a closed one whose generation still matters.
        for (i, gen) in img.handle_generations.iter().enumerate() {
            match vm.handles.get_mut(i) {
                Some(e) => e.generation = *gen,
                None => vm.handles.push(HandleEntry {
                    generation: *gen,
                    host: None,
                }),
            }
        }
        vm.free_handles = img.free_handles.clone();
        vm.gensym = img.gensym;
        vm.out = img.out.clone();

        let mut ex = Execution::new();
        ex.slots = img.slots.iter().map(&get).collect();
        ex.fuel = img.fuel;
        ex.frames = img
            .frames
            .iter()
            .map(|f| Frame {
                proto: f.proto,
                pc: f.pc,
                base: f.base,
                dst: f.dst,
                ret_len: f.ret_len,
                closure: match &objects[f.closure as usize] {
                    Value::Fn(c) => c.clone(),
                    _ => unreachable!("a frame's closure id names a closure"),
                },
            })
            .collect();
        ex.handlers = img
            .handlers
            .iter()
            .map(|h| Handler {
                frame: h.frame,
                target: h.target,
                err: h.err,
            })
            .collect();
        ex.pending = img
            .pending
            .iter()
            .map(|p| Pending {
                depth: p.depth,
                unwind: Unwind {
                    value: get(&p.value),
                    origin: p.origin,
                    suppressed: p.suppressed.iter().map(&get).collect(),
                },
            })
            .collect();
        Ok((vm, ex))
    }

    fn deref(r: &Ref, objects: &[Value]) -> Value {
        match r {
            Ref::Nil => Value::Nil,
            Ref::Bool(b) => Value::Bool(*b),
            Ref::Int(i) => Value::Int(*i),
            Ref::Float(bits) => Value::Float(f64::from_bits(*bits)),
            Ref::Sym(s) => Value::Sym(SymId(*s)),
            Ref::Keyword(k) => Value::Keyword(KwId(*k)),
            Ref::Cell(i, g) => Value::Cell(CellId(*i, *g)),
            Ref::Handle(i, g) => Value::Handle(HandleId(*i, *g)),
            Ref::Buffer(i, g) => Value::Buffer(BufferId(*i, *g)),
            Ref::Obj(id) => objects[*id as usize].clone(),
        }
    }
}

// ---------------------------------------------------------------------------

/// A REPL session: one `Vm`, one `Chunk`, one macro table, one gensym counter
/// (ADR-044).
///
/// The semantics live here and the prompt lives in `src/main.rs`, per ADR-031 —
/// a change in this module can change what a program means, and a change there
/// may not. That split is why the session is testable without a terminal.
/// Host adapters (ADR-045). File modules rather than inline `mod` blocks, and
/// **outside the line budget** — `BUILD.md` draws that boundary because
/// substantial host capability is a Rust library behind the handle table, not a
/// language subsystem. The budget test excludes this directory by path and
/// prints what it excluded, so the exclusion cannot be used to hide growth.
pub mod adapters;

pub mod session {
    use crate::bytecode::Chunk;
    use crate::error::LispErr;
    use crate::expand::Macros;
    use crate::value::Value;
    use crate::vm::{Outcome, Unwind, Vm};
    use crate::{compile, expand, reader, vm};

    /// How one input ended. Not `Outcome`: a session never suspends, because
    /// nothing fuels it, and an input that fails to *read* has not run at all —
    /// which is a different thing from one that ran and threw.
    pub enum Ended {
        Value(Value),
        Threw(Unwind),
    }

    pub struct Session {
        pub vm: Vm,
        /// One chunk for the whole session, extended per input. Existing proto
        /// indices never move, so a closure defined three inputs ago still
        /// names a valid proto (ADR-044 parts 2 and 3).
        chunk: Chunk,
        macros: Macros,
    }

    impl Session {
        pub fn new() -> Session {
            let mut vm = Vm::new();
            // Once for the session, not once per input. The gensym counter is
            // reset here and never again, so no two inputs can mint the same
            // fresh name.
            vm.reset_gensym();
            let macros = Macros::with_prelude(&mut vm);
            Session {
                vm,
                chunk: Chunk { protos: Vec::new() },
                macros,
            }
        }

        /// Read, expand, compile into the session's chunk, and run what was
        /// added. The `Err` side is a failure to *build* the input; a program
        /// that ran and threw comes back as `Ok(Ended::Threw)`, because that is
        /// a result and not a malformed input.
        pub fn eval(&mut self, src: &str) -> Result<Ended, LispErr> {
            let forms = reader::read_all(src, &mut self.vm.interner)?;
            let forms = expand::expand_in(forms, &mut self.vm, &mut self.macros)?;
            let top = compile::compile_into(&mut self.chunk, &forms, &mut self.vm.interner)?;
            let ex = vm::start_at(&self.chunk, top);
            let (outcome, _, _) = vm::run_fueled(&mut self.vm, &self.chunk, ex, u64::MAX);
            Ok(match outcome {
                Outcome::Returned(v) => Ended::Value(v),
                Outcome::Threw(u) => Ended::Threw(u),
                // Nothing fuels a session. ADR-043 part 4 makes suspension a
                // library facility for the round-trip property, and a prompt
                // that could suspend would need somewhere to put the
                // `Execution` — which is ADR-044's open clause, not this.
                Outcome::Suspended => unreachable!("a session runs un-fuelled"),
            })
        }

        /// Everything the input emitted. Drained per input, so output arrives
        /// interleaved with prompts rather than at the end of the session.
        pub fn take_output(&mut self) -> String {
            self.vm.take_output()
        }
    }

    impl Default for Session {
        fn default() -> Session {
            Session::new()
        }
    }

    /// Is this buffer a complete input, or did the user stop mid-form?
    ///
    /// A flag on the reader's error rather than a second parser or a delimiter
    /// count (ADR-044 part 5). Counting brackets gets strings and comments
    /// wrong, and the reader already knows the answer exactly — the only thing
    /// missing was a way to ask that did not involve matching on prose.
    /// Reads into a throwaway interner, never the session's. An abandoned line
    /// would otherwise leave its symbols interned — harmless to run, but they
    /// reach an `Image`, and a session's symbol table should hold what was
    /// evaluated rather than what was typed.
    pub fn wants_more(src: &str) -> bool {
        match reader::read_all(src, &mut crate::value::Interner::new()) {
            Err(e) => e.truncated,
            Ok(_) => false,
        }
    }
}
