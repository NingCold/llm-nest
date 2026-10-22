use crate::{Result, SocialFeature, wire};
impl SocialFeature {
    pub async fn reference_preview(
        &self,
        input: wire::ReferenceInput,
    ) -> Result<wire::ReferencePreview> {
        let s = self.services()?;
        let preview = s
            .resources
            .store
            .reference_preview(s.human, input.actor_id, input.selection)
            .await?;
        Ok(wire::ReferencePreview {
            conversation_id: preview.conversation.id,
            title: preview.conversation.title,
            messages: preview.messages.into_iter().map(Into::into).collect(),
            summary: preview.summary,
        })
    }
}
