use std::borrow::Borrow;
use std::env::VarError;

pub type ExpandError = shellexpand::LookupError<VarError>;

pub fn expand(input: &str) -> Result<String, ExpandError> {
    let result = shellexpand::env(input)?;
    let result: &str = result.borrow();
    Ok(String::from(result))
}
