package com.stellarmls.chat.onchain

import android.util.Base64
import org.json.JSONObject

/** On-chain stored state for a group. */
data class SEPCommitmentEntry(
    val commitment: ByteArray,
    val epoch: Long,
    val timestamp: Long,
    val tier: Int,
    val active: Boolean
) {
    companion object {
        fun fromJson(json: JSONObject) = SEPCommitmentEntry(
            commitment = Base64.decode(json.getString("commitment"), Base64.NO_WRAP),
            epoch = json.getLong("epoch"),
            timestamp = json.getLong("timestamp"),
            tier = json.getInt("tier"),
            active = json.getBoolean("active")
        )
    }
}

/** Response from create_group, update_commitment, deactivate_group. */
data class SEPSubmissionResponse(
    val accepted: Boolean,
    val transactionHash: String? = null,
    val message: String? = null
) {
    companion object {
        fun fromJson(json: JSONObject) = SEPSubmissionResponse(
            accepted = json.getBoolean("accepted"),
            transactionHash = if (json.has("transactionHash")) json.getString("transactionHash") else null,
            message = if (json.has("message")) json.getString("message") else null
        )
    }
}

/** Response from verify_membership. */
data class SEPVerifyMembershipResponse(
    val valid: Boolean
) {
    companion object {
        fun fromJson(json: JSONObject) = SEPVerifyMembershipResponse(
            valid = json.getBoolean("valid")
        )
    }
}

/** JSON builder for create_group request payload. */
fun buildCreateGroupPayload(
    groupID: ByteArray,
    commitment: ByteArray,
    proof: ByteArray,
    commitmentForInputs: ByteArray,
    epoch: Long,
    tier: Int
): JSONObject = JSONObject().apply {
    put("groupID", groupID.toBase64())
    put("commitment", commitment.toBase64())
    put("proof", proof.toBase64())
    put("publicInputs", JSONObject().apply {
        put("commitment", commitmentForInputs.toBase64())
        put("epoch", epoch)
    })
    put("tier", tier)
}

/** JSON builder for update_commitment request payload. */
fun buildUpdateCommitmentPayload(
    groupID: ByteArray,
    newCommitment: ByteArray,
    newEpoch: Long,
    proof: ByteArray,
    oldCommitment: ByteArray,
    oldEpoch: Long
): JSONObject = JSONObject().apply {
    put("groupID", groupID.toBase64())
    put("newCommitment", newCommitment.toBase64())
    put("newEpoch", newEpoch)
    put("proof", proof.toBase64())
    put("publicInputs", JSONObject().apply {
        put("commitment", oldCommitment.toBase64())
        put("epoch", oldEpoch)
    })
}

/** JSON builder for verify_membership request payload. */
fun buildVerifyMembershipPayload(
    groupID: ByteArray,
    proof: ByteArray,
    commitment: ByteArray,
    epoch: Long
): JSONObject = JSONObject().apply {
    put("groupID", groupID.toBase64())
    put("proof", proof.toBase64())
    put("publicInputs", JSONObject().apply {
        put("commitment", commitment.toBase64())
        put("epoch", epoch)
    })
}

/** JSON builder for get_state request payload. */
fun buildGetStatePayload(groupID: ByteArray): JSONObject = JSONObject().apply {
    put("groupID", groupID.toBase64())
}

private fun ByteArray.toBase64(): String = Base64.encodeToString(this, Base64.NO_WRAP)
