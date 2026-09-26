//! The `if` expression language the toolchain's declarations are written in.
//!
//! Every condition in a declaration - the keys of the `channels` table, the
//! `if` of a permutation choice, the `if` of a pass - is an expression over the
//! permutation macros. The grammar, as the files use it:
//!
//! ```text
//! defined(HAS_BASE_COLOR)
//! !defined(HAS_NORMAL)
//! defined(A) && defined(B)
//! (defined(HAS_NORMAL) && !defined(WORLD_SPACE_NORMAL)) || defined(NEEDS_TANGENT_SPACE)
//! num_skin_weights() == 4
//! on_platform(GL)
//! on_renderer(D3D11, D3D12, GNM, GL)
//! ```
//!
//! Evaluation is three-valued. A macro test is answered by the defines of the
//! permutation, but a call is an *engine* query - how many skin weights a mesh
//! has, which renderer is running - and a generated declaration knows nothing of it.
//! So a query evaluates to `None` rather than to a guess, and the combinators
//! fold that through Kleene logic: `None && false` is `false`, `None || true` is
//! `true`, and anything else stays unknown. A caller that needs a decision - the
//! channel list of a group, say - gets `None` when the expression reaches a
//! fact it cannot answer, instead of a wrong answer.

use std::collections::BTreeSet;

use color_eyre::eyre::{Result, bail};

/// The macros one permutation defines.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Defines(BTreeSet<String>);

impl Defines {
    /// The macros of a list of names, as a permutation records them.
    pub fn new(macros: impl IntoIterator<Item = String>) -> Self {
        Self(macros.into_iter().collect())
    }

    /// Whether these defines hold `condition`, or `None` when the condition asks
    /// something only the engine can answer.
    pub fn holds(&self, condition: &Condition) -> Option<bool> {
        condition.holds(self)
    }

    /// The macros themselves.
    pub fn macros(&self) -> impl Iterator<Item = &String> {
        self.0.iter()
    }

    /// Whether a macro of that name is defined.
    pub fn has(&self, macro_name: &str) -> bool {
        self.0.contains(macro_name)
    }
}

impl FromIterator<String> for Defines {
    fn from_iter<T: IntoIterator<Item = String>>(iter: T) -> Self {
        Self::new(iter)
    }
}

/// A parsed condition.
#[derive(Clone, Debug, PartialEq)]
pub struct Condition(Term);

impl Condition {
    /// Parses an expression.
    pub fn parse(text: &str) -> Result<Self> {
        let mut parser = Parser {
            chars: text.chars().collect(),
            at: 0,
        };
        let term = parser.expression()?;
        parser.skip();
        if parser.peek().is_some() {
            bail!("unexpected {:?} at character {}", parser.peek(), parser.at);
        }
        Ok(Self(term))
    }

    /// Whether the defines hold the condition, or `None` when it asks something
    /// only the engine can answer.
    pub fn holds(&self, defines: &Defines) -> Option<bool> {
        self.0.holds(defines)
    }

    /// The term, for a caller that wants to walk the expression itself.
    pub fn term(&self) -> &Term {
        &self.0
    }
}

/// One node of a condition.
#[derive(Clone, Debug, PartialEq)]
pub enum Term {
    /// `A`, or `defined(A)`: a macro of that name is defined.
    Defined(String),
    /// `!term`.
    Not(Box<Term>),
    /// `a && b`, flattened.
    All(Vec<Term>),
    /// `a || b`, flattened.
    Any(Vec<Term>),
    /// A call the defines cannot answer: `instanced()`, `on_platform(GL)`. The
    /// arguments are the words inside the call.
    Query(Query),
    /// A comparison the defines cannot answer: `num_skin_weights() == 4`.
    Compare {
        /// The call on the left.
        left: Query,
        /// Whether the words have to differ instead.
        negated: bool,
        /// The word on the right.
        right: String,
    },
}

/// A call in a condition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Query {
    /// The name of the query, as it is spelled.
    pub name: String,
    /// Its arguments, in order.
    pub args: Vec<String>,
}

impl Term {
    /// Whether the defines hold the term, or `None` when it asks something only
    /// the engine can answer.
    pub fn holds(&self, defines: &Defines) -> Option<bool> {
        match self {
            Self::Defined(name) => Some(defines.has(name)),
            Self::Not(term) => term.holds(defines).map(|held| !held),
            // Kleene logic: a known answer wins over an unknown one.
            Self::All(terms) => {
                let mut unknown = false;
                for term in terms {
                    match term.holds(defines) {
                        Some(false) => return Some(false),
                        Some(true) => {}
                        None => unknown = true,
                    }
                }
                (!unknown).then_some(true)
            }
            Self::Any(terms) => {
                let mut unknown = false;
                for term in terms {
                    match term.holds(defines) {
                        Some(true) => return Some(true),
                        Some(false) => {}
                        None => unknown = true,
                    }
                }
                (!unknown).then_some(false)
            }
            Self::Query(_) | Self::Compare { .. } => None,
        }
    }
}

/// A cursor over the expression.
struct Parser {
    chars: Vec<char>,
    at: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.at).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let char = self.peek();
        if char.is_some() {
            self.at += 1;
        }
        char
    }

    fn skip(&mut self) {
        while matches!(self.peek(), Some(char) if char.is_whitespace()) {
            self.at += 1;
        }
    }

    /// `a || b || c`
    fn expression(&mut self) -> Result<Term> {
        let mut terms = vec![self.conjunction()?];
        loop {
            self.skip();
            if self.takes("||") {
                terms.push(self.conjunction()?);
            } else {
                break;
            }
        }
        Ok(match terms.len() {
            1 => terms.pop().expect("one term"),
            _ => Term::Any(terms),
        })
    }

    /// `a && b && c`
    fn conjunction(&mut self) -> Result<Term> {
        let mut terms = vec![self.unary()?];
        loop {
            self.skip();
            if self.takes("&&") {
                terms.push(self.unary()?);
            } else {
                break;
            }
        }
        Ok(match terms.len() {
            1 => terms.pop().expect("one term"),
            _ => Term::All(terms),
        })
    }

    /// `!term`, or a term in brackets.
    fn unary(&mut self) -> Result<Term> {
        self.skip();
        if self.takes("!") {
            return Ok(Term::Not(Box::new(self.unary()?)));
        }
        if self.takes("(") {
            let term = self.expression()?;
            self.skip();
            if !self.takes(")") {
                bail!("a bracket is not closed, at character {}", self.at);
            }
            return Ok(term);
        }
        self.atom()
    }

    /// A macro test, a call, or a comparison.
    fn atom(&mut self) -> Result<Term> {
        let name = self.word()?;
        self.skip();
        // `defined(X)` is the one call the defines answer, so it is not a query.
        if name == "defined" && self.peek() == Some('(') {
            self.at += 1;
            self.skip();
            let macro_name = self.word()?;
            self.skip();
            if !self.takes(")") {
                bail!("defined() is not closed, at character {}", self.at);
            }
            return Ok(Term::Defined(macro_name));
        }
        if self.peek() != Some('(') {
            // A bare name is a macro test, as `defined(name)` would be.
            return Ok(Term::Defined(name));
        }
        self.at += 1;
        let mut args = Vec::new();
        loop {
            self.skip();
            if self.takes(")") {
                break;
            }
            args.push(self.word()?);
            self.skip();
            if self.takes(",") {
                continue;
            }
            if self.takes(")") {
                break;
            }
            bail!("expected `,` or `)` in a call, at character {}", self.at);
        }
        let left = Query { name, args };
        self.skip();
        let negated = if self.takes("==") {
            false
        } else if self.takes("!=") {
            true
        } else {
            // A call on its own is a question with a boolean answer.
            return Ok(Term::Query(left));
        };
        self.skip();
        let right = self.word()?;
        Ok(Term::Compare {
            left,
            negated,
            right,
        })
    }

    /// A word: an identifier, a number, or anything else up to whitespace or a
    /// structural character.
    fn word(&mut self) -> Result<String> {
        self.skip();
        let start = self.at;
        while matches!(self.peek(), Some(char) if char.is_alphanumeric() || matches!(char, '_' | '.'))
        {
            self.at += 1;
        }
        if start == self.at {
            bail!(
                "expected a name at character {}, found {:?}",
                self.at,
                self.peek()
            );
        }
        Ok(self.chars[start..self.at].iter().collect())
    }

    /// Whether the text at the cursor starts with `token`, consuming it.
    fn takes(&mut self, token: &str) -> bool {
        self.skip();
        let token: Vec<char> = token.chars().collect();
        if self.chars[self.at..].starts_with(&token) {
            self.at += token.len();
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defines(macros: &[&str]) -> Defines {
        Defines::new(macros.iter().map(|name| name.to_string()))
    }

    fn holds(text: &str, macros: &[&str]) -> Option<bool> {
        Condition::parse(text).expect(text).holds(&defines(macros))
    }

    #[test]
    fn a_macro_test_is_answered_by_the_defines() {
        assert_eq!(holds("defined(A)", &["A"]), Some(true));
        assert_eq!(holds("defined(A)", &[]), Some(false));
        assert_eq!(holds("defined(A)", &["B"]), Some(false));
        // The two spellings of the same test.
        assert_eq!(holds("A", &["A"]), Some(true));
        assert_eq!(holds("!defined(A)", &["A"]), Some(false));
        assert_eq!(holds("!defined(A)", &[]), Some(true));
    }

    #[test]
    fn combinators_fold_the_known_answers() {
        assert_eq!(holds("defined(A) && defined(B)", &["A", "B"]), Some(true));
        assert_eq!(holds("defined(A) && defined(B)", &["A"]), Some(false));
        assert_eq!(holds("defined(A) || defined(B)", &["A"]), Some(true));
        assert_eq!(holds("defined(A) || defined(B)", &["C"]), Some(false));
        // Any length.
        assert_eq!(
            holds("defined(A) && defined(B) && defined(C)", &["A", "C"]),
            Some(false)
        );
        assert_eq!(
            holds("defined(A) || defined(B) || defined(C)", &["C"]),
            Some(true)
        );
    }

    #[test]
    fn brackets_group_the_operators() {
        let text = "(defined(A) || defined(B)) && defined(C)";
        assert_eq!(holds(text, &["A", "C"]), Some(true));
        assert_eq!(holds(text, &["A"]), Some(false));
        assert_eq!(holds(text, &["B", "C"]), Some(true));
        // Without the brackets the last `&&` would bind the whole thing.
        assert_eq!(
            holds("defined(A) || defined(B) && defined(C)", &["A"]),
            Some(true)
        );
        assert_eq!(
            holds("defined(A) || defined(B) && defined(C)", &["D"]),
            Some(false)
        );
    }

    #[test]
    fn a_query_is_unknown_and_its_neighbours_win() {
        assert_eq!(holds("instanced()", &[]), None);
        assert_eq!(holds("!instanced()", &[]), None);
        // A known answer beats an unknown one.
        assert_eq!(holds("defined(A) || instanced()", &["A"]), Some(true));
        assert_eq!(holds("defined(A) && instanced()", &["A"]), None);
        assert_eq!(holds("defined(A) && instanced()", &[]), Some(false));
        assert_eq!(holds("!defined(A) && instanced()", &[]), None);
        assert_eq!(holds("!defined(A) || instanced()", &[]), Some(true));
        assert_eq!(holds("!defined(A) || instanced()", &["A"]), None);
        // Two unknowns stay unknown.
        assert_eq!(holds("instanced() && on_platform(GL)", &[]), None);
    }

    #[test]
    fn a_comparison_is_unknown_too() {
        assert_eq!(holds("num_skin_weights() == 4", &[]), None);
        assert_eq!(
            holds("num_skin_weights() != 4", &["SKINNED_4WEIGHTS"]),
            None
        );
        // But it is still unknown *as a question*, so `defined` around it wins.
        assert_eq!(
            holds(
                "lightmap_format() == directional_irradiance || defined(A)",
                &["A"]
            ),
            Some(true)
        );
    }

    #[test]
    fn calls_keep_their_arguments() {
        let condition = Condition::parse("on_renderer(D3D11, D3D12, GNM, GL)").expect("parse");
        let Term::Query(query) = condition.term() else {
            panic!("expected a query, got {:?}", condition.term());
        };
        assert_eq!(query.name, "on_renderer");
        assert_eq!(query.args, ["D3D11", "D3D12", "GNM", "GL"]);
    }

    #[test]
    fn reads_a_condition_off_a_real_declaration() {
        // The tangent-space condition of the anisotropic base node.
        let text = "(defined(HAS_NORMAL) && !defined(WORLD_SPACE_NORMAL)) || \
                    defined(NEEDS_TANGENT_SPACE) || defined(HAS_ANISOTROPY)";
        assert_eq!(holds(text, &[]), Some(false));
        assert_eq!(holds(text, &["HAS_NORMAL"]), Some(true));
        // The normal map is world space, so the tangent basis is not needed.
        assert_eq!(
            holds(text, &["HAS_NORMAL", "WORLD_SPACE_NORMAL"]),
            Some(false)
        );
        assert_eq!(
            holds(text, &["HAS_NORMAL", "NEEDS_TANGENT_SPACE"]),
            Some(true)
        );
        assert_eq!(holds(text, &["HAS_ANISOTROPY"]), Some(true));
    }

    #[test]
    fn whitespace_does_not_matter() {
        assert_eq!(
            holds("  defined( A )\t&&\ndefined( B ) ", &["A", "B"]),
            Some(true)
        );
    }

    #[test]
    fn reports_what_it_cannot_parse() {
        for text in [
            "",
            "defined(",
            "defined(A",
            "(defined(A)",
            "defined(A) &&",
            "&& defined(A)",
            "defined(A) defined(B)",
            "!",
        ] {
            assert!(Condition::parse(text).is_err(), "{text:?} must not parse");
        }
    }
}
