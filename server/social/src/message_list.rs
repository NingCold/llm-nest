use crate::{Result, SocialFeature, wire};
use social::{ConversationId, MessageId};
use storage::social::{ConversationListState, SearchMessages, UpdateConversationList};

impl SocialFeature {
    pub async fn update_conversation_list(
        &self,
        id: ConversationId,
        input: wire::ConversationListInput,
    ) -> Result<ConversationListState> {
        Ok(self
            .services()?
            .resources
            .store
            .update_conversation_list(
                self.context(input.request_id)?,
                UpdateConversationList {
                    conversation_id: id,
                    expected_revision: input.expected_revision,
                    action: input.action,
                },
            )
            .await?)
    }
    pub async fn search_messages(&self, input: SearchMessages) -> Result<wire::MessageSearchPage> {
        let s = self.services()?;
        let page = s.resources.store.search_messages(s.human, input).await?;
        Ok(wire::MessageSearchPage {
            messages: page.hits.into_iter().map(|h| h.message.into()).collect(),
            next_before: page.next_before,
        })
    }
    pub async fn message_context(&self, id: MessageId) -> Result<wire::MessageContext> {
        let s = self.services()?;
        let page = s.resources.store.message_context(s.human, id).await?;
        Ok(wire::MessageContext {
            messages: page.messages.into_iter().map(Into::into).collect(),
            has_more: page.has_more,
            has_newer: page.has_newer,
        })
    }
}
