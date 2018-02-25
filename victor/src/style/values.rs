use cssparser::{Parser, Token};
use primitives::{CssPx, Length as EuclidLength};
use style::errors::{PropertyParseError, PropertyParseErrorKind};

/// <https://drafts.csswg.org/css-values/#lengths>
pub enum Length {
    Px(EuclidLength<CssPx>)
}

impl Length {
    pub fn parse<'i, 't>(parser: &mut Parser<'i, 't>) -> Result<Self, PropertyParseError<'i>> {
        match *parser.next()? {
            Token::Dimension { value, ref unit, .. } => match_ignore_ascii_case!(unit,
                "px" => return Ok(Length::Px(EuclidLength::new(value))),
                _ => {}
            ),
            _ => {}
        }
        Err(parser.new_custom_error(PropertyParseErrorKind::Other))
    }
}