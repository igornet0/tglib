use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TelegramAccountPermissions {
    pub read_messages: bool,
    pub send_messages: bool,
    pub forward_messages: bool,
    pub edit_messages: bool,
    pub delete_messages: bool,
    pub manage_chats: bool,
}

impl Default for TelegramAccountPermissions {
    fn default() -> Self {
        Self {
            read_messages: true,
            send_messages: true,
            forward_messages: true,
            edit_messages: true,
            delete_messages: true,
            manage_chats: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TelegramActionKind {
    ReadMessages,
    SendMessage,
    ForwardMessage,
    EditMessage,
    DeleteMessages,
    ManageChats,
}

impl TelegramActionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadMessages => "read_messages",
            Self::SendMessage => "send_message",
            Self::ForwardMessage => "forward_message",
            Self::EditMessage => "edit_message",
            Self::DeleteMessages => "delete_messages",
            Self::ManageChats => "manage_chats",
        }
    }
}

impl TelegramAccountPermissions {
    pub fn allows(&self, action: TelegramActionKind) -> bool {
        match action {
            TelegramActionKind::ReadMessages => self.read_messages,
            TelegramActionKind::SendMessage => self.send_messages,
            TelegramActionKind::ForwardMessage => self.forward_messages,
            TelegramActionKind::EditMessage => self.edit_messages,
            TelegramActionKind::DeleteMessages => self.delete_messages,
            TelegramActionKind::ManageChats => self.manage_chats,
        }
    }
}
