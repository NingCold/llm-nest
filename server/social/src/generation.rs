use crate::{Result, wire};
pub(crate) use builtin_agent::generation::limits;

pub(crate) fn options(
    binding: &wire::Binding,
    resolved: &ai_client::ResolvedSelection,
) -> Result<common::GenerationOptions> {
    Ok(builtin_agent::generation::options(
        &builtin_agent::generation::Parameters {
            temperature: binding.temperature,
            max_tokens: binding.max_tokens,
        },
        resolved,
    )?)
}
