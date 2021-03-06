use crate::primitives::{CssPx, Length as EuclidLength};
use crate::style::errors::{PropertyParseError, PropertyParseErrorKind};
use crate::style::values::{Parse, ToComputedValue};
use cssparser::{Parser, Token};

pub type PxLength = EuclidLength<CssPx>;

/// <https://drafts.csswg.org/css-values/#lengths>
#[derive(Copy, Clone)]
pub enum Length {
    Px(PxLength),
}

impl Parse for Length {
    fn parse<'i, 't>(parser: &mut Parser<'i, 't>) -> Result<Self, PropertyParseError<'i>> {
        match *parser.next()? {
            Token::Dimension {
                value, ref unit, ..
            } => match_ignore_ascii_case!(unit,
                "px" => return Ok(Length::Px(PxLength::new(value))),
                _ => {}
            ),
            _ => {}
        }
        Err(parser.new_custom_error(PropertyParseErrorKind::Other))
    }
}

impl ToComputedValue for Length {
    type Computed = PxLength;
    fn to_computed(&self) -> Self::Computed {
        match *self {
            Length::Px(px) => px,
        }
    }
}
