package com.stellarmls.chat.persistence

import android.content.Context
import com.stellarmls.chat.crypto.StorageEncryption
import com.stellarmls.chat.model.ChatGroup
import com.stellarmls.chat.model.ChatMessage
import com.stellarmls.chat.model.EpochSnapshot
import com.stellarmls.chat.model.toHex
import com.stellarmls.mls.SEPGroupMemberLeaf
import com.stellarmls.mls.SEPTier
import org.json.JSONArray
import org.json.JSONObject
import java.util.Date

class PersistenceStore(context: Context) {
    internal val dao: StellarChatDao

    init {
        StorageEncryption.init(context)
        dao = StellarChatDatabase.getInstance(context).dao()
    }

    suspend fun loadGroups(): List<ChatGroup> {
        return dao.loadGroups().map { decryptGroup(it) }
    }

    suspend fun saveGroup(group: ChatGroup) {
        dao.saveGroup(encryptGroup(group))
    }

    suspend fun deleteGroup(id: String) {
        dao.deleteGroup(id)
        dao.deleteMessages(id)
        dao.deleteEpochSnapshots(id)
    }

    suspend fun loadMessages(groupID: String): List<ChatMessage> {
        return dao.loadMessages(groupID).map { decryptMessage(it) }
    }

    suspend fun saveMessage(message: ChatMessage) {
        dao.saveMessage(encryptMessage(message))
    }

    // -- Transport Bundles --

    suspend fun loadTransportBundles(groupID: String): Map<String, com.stellarmls.mls.SEPMemberTransportBundle> {
        val result = mutableMapOf<String, com.stellarmls.mls.SEPMemberTransportBundle>()
        for (persisted in dao.loadTransportBundles(groupID)) {
            try {
                val json = String(StorageEncryption.decrypt(persisted.encryptedBundle))
                val bundle = deserializeTransportBundle(json)
                result[persisted.blsPubkeyHex] = bundle
            } catch (_: Exception) { }
        }
        return result
    }

    suspend fun saveTransportBundle(bundle: com.stellarmls.mls.SEPMemberTransportBundle, groupID: String) {
        val json = serializeTransportBundle(bundle)
        val encrypted = StorageEncryption.encrypt(json.toByteArray())
        val hex = bundle.blsPubkey.toHex()
        dao.saveTransportBundle(PersistedTransportBundle(groupID, hex, encrypted))
    }

    suspend fun deleteTransportBundles(groupID: String) {
        dao.deleteTransportBundles(groupID)
    }

    // -- Pending Rekeys --

    suspend fun savePendingRekey(
        groupID: String, epoch: Int, envelopeJson: String,
        unackedKeys: List<String>, isRemovalEpoch: Boolean
    ) {
        val encrypted = StorageEncryption.encrypt(envelopeJson.toByteArray())
        dao.savePendingRekey(PersistedPendingRekey(
            id = "$groupID-$epoch",
            groupID = groupID,
            epoch = epoch,
            encryptedEnvelope = encrypted,
            unackedMemberKeysJSON = org.json.JSONArray(unackedKeys).toString(),
            isRemovalEpoch = isRemovalEpoch
        ))
    }

    suspend fun loadPendingRekeys(groupID: String): List<PersistedPendingRekey> {
        return dao.loadPendingRekeys(groupID)
    }

    suspend fun deletePendingRekey(groupID: String, epoch: Int) {
        dao.deletePendingRekey(groupID, epoch)
    }

    /** Remove a single member from the unacked set. Deletes the record if all members have acked. */
    suspend fun updatePendingRekeyAck(groupID: String, epoch: Int, ackedMemberHex: String) {
        val records = dao.loadPendingRekeys(groupID)
        val record = records.firstOrNull { it.epoch == epoch } ?: return
        val keys = try {
            val arr = org.json.JSONArray(record.unackedMemberKeysJSON)
            (0 until arr.length()).map { arr.getString(it) }.toMutableList()
        } catch (_: Exception) { mutableListOf() }
        keys.removeAll { it == ackedMemberHex }
        if (keys.isEmpty()) {
            dao.deletePendingRekey(groupID, epoch)
        } else {
            dao.updatePendingRekeyUnacked(groupID, epoch, org.json.JSONArray(keys).toString())
        }
    }

    suspend fun incrementPendingRekeyRetry(groupID: String, epoch: Int) {
        dao.incrementPendingRekeyRetry(groupID, epoch)
    }

    suspend fun loadAllPendingRekeys(): List<PersistedPendingRekey> {
        return dao.loadAllPendingRekeys()
    }

    suspend fun isRemovalEpoch(groupID: String, epoch: Int): Boolean {
        return dao.isRemovalEpoch(groupID, epoch) > 0
    }

    // -- Transport Bundle Serialization --

    private fun serializeTransportBundle(bundle: com.stellarmls.mls.SEPMemberTransportBundle): String {
        return com.stellarmls.chat.model.BootstrapPayload.serializeTransportBundle(bundle).toString()
    }

    private fun deserializeTransportBundle(json: String): com.stellarmls.mls.SEPMemberTransportBundle {
        return com.stellarmls.chat.model.BootstrapPayload.deserializeTransportBundle(org.json.JSONObject(json))
    }

    // -- Encryption helpers --

    private fun encryptGroup(group: ChatGroup): PersistedGroup {
        val membersJson = serializeMembers(group.members)
        return PersistedGroup(
            id = group.id,
            encryptedName = StorageEncryption.encrypt(group.name),
            encryptedGroupSecret = StorageEncryption.encrypt(group.groupSecret),
            createdAt = group.createdAt.time,
            relayHintsJSON = JSONArray(group.relayHints).toString(),
            encryptedMembers = StorageEncryption.encrypt(membersJson.toByteArray()),
            epoch = group.epoch.toInt(),
            encryptedSalt = StorageEncryption.encrypt(group.salt),
            encryptedCommitment = group.commitment?.let { StorageEncryption.encrypt(it) },
            tierRawValue = group.tier.id,
            isPublishedOnChain = group.isPublishedOnChain,
            pinnedEpoch = group.pinnedEpoch?.toInt()
        )
    }

    private fun decryptGroup(persisted: PersistedGroup): ChatGroup {
        val name = StorageEncryption.decryptString(persisted.encryptedName)
        val groupSecret = StorageEncryption.decrypt(persisted.encryptedGroupSecret)
        val membersJson = String(StorageEncryption.decrypt(persisted.encryptedMembers))
        val salt = StorageEncryption.decrypt(persisted.encryptedSalt)
        val commitment = persisted.encryptedCommitment?.let { StorageEncryption.decrypt(it) }
        val relayHints = JSONArray(persisted.relayHintsJSON).let { arr ->
            (0 until arr.length()).map { arr.getString(it) }
        }
        val members = deserializeMembers(membersJson)
        val tier = SEPTier.entries.find { it.id == persisted.tierRawValue } ?: SEPTier.LARGE

        return ChatGroup(
            id = persisted.id,
            name = name,
            groupSecret = groupSecret,
            createdAt = Date(persisted.createdAt),
            relayHints = relayHints,
            members = members.toMutableList(),
            epoch = persisted.epoch.toLong(),
            salt = salt,
            commitment = commitment,
            tier = tier,
            isPublishedOnChain = persisted.isPublishedOnChain,
            pinnedEpoch = persisted.pinnedEpoch?.toLong()
        )
    }

    private fun encryptMessage(message: ChatMessage): PersistedMessage {
        val encMedia = message.mediaAttachment?.let { media ->
            val json = serializeMediaAttachment(media)
            StorageEncryption.encrypt(json.toByteArray())
        }
        return PersistedMessage(
            id = message.id,
            groupID = message.groupID,
            senderPubkey = message.senderPubkey,
            encryptedText = StorageEncryption.encrypt(message.text),
            timestamp = message.timestamp.time,
            isMine = message.isMine,
            encryptedMediaAttachment = encMedia,
            isSystemMessage = message.isSystemMessage,
            epoch = message.epoch?.toInt()
        )
    }

    private fun decryptMessage(persisted: PersistedMessage): ChatMessage {
        val media = persisted.encryptedMediaAttachment?.let { enc ->
            try {
                val json = String(StorageEncryption.decrypt(enc))
                deserializeMediaAttachment(json)
            } catch (_: Exception) { null }
        }
        return ChatMessage(
            id = persisted.id,
            groupID = persisted.groupID,
            senderPubkey = persisted.senderPubkey,
            text = StorageEncryption.decryptString(persisted.encryptedText),
            timestamp = Date(persisted.timestamp),
            isMine = persisted.isMine,
            mediaAttachment = media,
            isSystemMessage = persisted.isSystemMessage,
            epoch = persisted.epoch?.toLong()
        )
    }

    private fun serializeMediaAttachment(media: com.stellarmls.chat.model.MediaAttachment): String {
        return JSONObject().apply {
            put("blobHash", media.blobHash)
            put("fileKey", android.util.Base64.encodeToString(media.fileKey, android.util.Base64.NO_WRAP))
            put("mimeType", media.mimeType)
            put("width", media.width)
            put("height", media.height)
            put("size", media.size)
            put("blossomServers", JSONArray(media.blossomServers))
            media.encryptedThumbnail?.let {
                put("encryptedThumbnail", android.util.Base64.encodeToString(it, android.util.Base64.NO_WRAP))
            }
            media.duration?.let { put("duration", it) }
        }.toString()
    }

    private fun deserializeMediaAttachment(json: String): com.stellarmls.chat.model.MediaAttachment {
        val obj = JSONObject(json)
        val servers = obj.getJSONArray("blossomServers").let { arr ->
            (0 until arr.length()).map { arr.getString(it) }
        }
        return com.stellarmls.chat.model.MediaAttachment(
            blobHash = obj.getString("blobHash"),
            fileKey = android.util.Base64.decode(obj.getString("fileKey"), android.util.Base64.NO_WRAP),
            mimeType = obj.getString("mimeType"),
            width = obj.getInt("width"),
            height = obj.getInt("height"),
            size = obj.getInt("size"),
            blossomServers = servers,
            encryptedThumbnail = obj.optString("encryptedThumbnail", "").takeIf { it.isNotEmpty() }
                ?.let { android.util.Base64.decode(it, android.util.Base64.NO_WRAP) },
            duration = obj.optInt("duration", -1).takeIf { it >= 0 }
        )
    }

    // -- Epoch Snapshot persistence --

    suspend fun saveEpochSnapshot(snapshot: EpochSnapshot, groupID: String) {
        val membersJson = serializeMembers(snapshot.members)
        dao.saveEpochSnapshot(PersistedEpochSnapshot(
            groupID = groupID,
            epoch = snapshot.epoch.toInt(),
            encryptedMembers = StorageEncryption.encrypt(membersJson.toByteArray()),
            encryptedSalt = StorageEncryption.encrypt(snapshot.salt),
            encryptedGroupSecret = StorageEncryption.encrypt(snapshot.groupSecret),
            changeDescription = snapshot.changeDescription
        ))
    }

    suspend fun loadEpochSnapshots(groupID: String): Map<Long, EpochSnapshot> {
        val result = mutableMapOf<Long, EpochSnapshot>()
        for (persisted in dao.loadEpochSnapshots(groupID)) {
            try {
                val members = deserializeMembers(
                    String(StorageEncryption.decrypt(persisted.encryptedMembers))
                )
                val salt = StorageEncryption.decrypt(persisted.encryptedSalt)
                val groupSecret = StorageEncryption.decrypt(persisted.encryptedGroupSecret)
                result[persisted.epoch.toLong()] = EpochSnapshot(
                    epoch = persisted.epoch.toLong(),
                    members = members,
                    salt = salt,
                    groupSecret = groupSecret,
                    changeDescription = persisted.changeDescription
                )
            } catch (_: Exception) { }
        }
        return result
    }

    suspend fun deleteEpochSnapshots(groupID: String) {
        dao.deleteEpochSnapshots(groupID)
    }

    /** Keep only the most recent [maxCount] snapshots for a group. */
    suspend fun trimEpochSnapshots(groupID: String, maxCount: Int) {
        val all = dao.loadEpochSnapshots(groupID) // ordered by epoch DESC
        if (all.size > maxCount) {
            // Delete all, then re-insert the most recent ones
            dao.deleteEpochSnapshots(groupID)
            for (snapshot in all.take(maxCount)) {
                dao.saveEpochSnapshot(snapshot)
            }
        }
    }

    // -- Member serialization (JSON with base64-encoded byte arrays) --

    private fun serializeMembers(members: List<SEPGroupMemberLeaf>): String {
        val arr = JSONArray()
        for (m in members) {
            val obj = JSONObject()
            obj.put("pk", android.util.Base64.encodeToString(m.publicKeyCompressed, android.util.Base64.NO_WRAP))
            obj.put("lh", android.util.Base64.encodeToString(m.leafHash, android.util.Base64.NO_WRAP))
            arr.put(obj)
        }
        return arr.toString()
    }

    private fun deserializeMembers(json: String): List<SEPGroupMemberLeaf> {
        val arr = JSONArray(json)
        return (0 until arr.length()).map { i ->
            val obj = arr.getJSONObject(i)
            SEPGroupMemberLeaf(
                publicKeyCompressed = android.util.Base64.decode(obj.getString("pk"), android.util.Base64.NO_WRAP),
                leafHash = android.util.Base64.decode(obj.getString("lh"), android.util.Base64.NO_WRAP)
            )
        }
    }
}
