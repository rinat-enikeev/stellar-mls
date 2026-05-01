package chat.onym.android.ui

object TestTags {
    object GroupList {
        const val Screen = "groupList.screen"
        const val Fab = "groupList.fab"
        const val InvitationsButton = "groupList.invitations"
        const val OnboardingSheet = "groupList.onboardingSheet"
        const val OnboardingPrimaryButton = "groupList.onboarding.primary"
        const val EmptyState = "groupList.empty"
        // Long-press context menu (Pin/Unpin shares one tag — only one is shown at a time).
        const val ContextMenu = "groupList.contextMenu"
        const val ContextMenuPin = "groupList.contextMenu.pin"
        const val ContextMenuDelete = "groupList.contextMenu.delete"
        const val ContextMenuCancel = "groupList.contextMenu.cancel"
        // Confirm-delete AlertDialog.
        const val DeleteDialogConfirm = "groupList.deleteDialog.confirm"
        const val DeleteDialogCancel = "groupList.deleteDialog.cancel"
        fun item(id: String) = "groupList.item.$id"
    }

    object Chat {
        const val Screen = "chat.screen"
        const val Back = "chat.back"
        const val GroupInfo = "chat.groupInfo"
        const val MessageInput = "chat.messageInput"
        const val SendButton = "chat.send"
    }

    object CreateGroup {
        const val Screen = "createGroup.screen"
        const val NameField = "createGroup.name"
        const val NextButton = "createGroup.next"
        const val CreateButton = "createGroup.create"
        const val CancelButton = "createGroup.cancel"
        const val BackButton = "createGroup.back"
    }

    object JoinGroup {
        const val Screen = "joinGroup.screen"
        const val InviteField = "joinGroup.invite"
        const val PasteButton = "joinGroup.paste"
        const val ScanButton = "joinGroup.scan"
    }

    object Settings {
        const val Screen = "settings.screen"
        const val InviteTab = "settings.tab.invite"
        const val PreferencesTab = "settings.tab.preferences"
        const val ShareLink = "settings.shareLink"
        const val CopyKey = "settings.copyKey"
    }

    object Search {
        const val Screen = "search.screen"
        const val Field = "search.field"
    }

    object Contacts {
        const val Screen = "contacts.screen"
    }

    object BottomNav {
        const val Bar = "bottomNav.bar"
        fun item(route: String) = "bottomNav.item.$route"
    }
}
