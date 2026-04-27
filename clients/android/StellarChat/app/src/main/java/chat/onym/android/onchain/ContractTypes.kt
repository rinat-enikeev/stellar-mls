package chat.onym.android.onchain

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

/** Response from create_group / update_commitment. */
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

/**
 * On-chain state for governance-aware groups (returned by `get_state_v2`).
 * Legacy V1 groups are projected by the contract as `groupType = 0` (Anarchy)
 * with `memberCount = 0`.
 */
data class SEPCommitmentEntryV2(
    val commitment: ByteArray,
    val epoch: Long,
    val timestamp: Long,
    val tier: Int,
    val active: Boolean,
    val groupType: Int,
    val memberCount: Int
) {
    companion object {
        fun fromJson(json: JSONObject) = SEPCommitmentEntryV2(
            commitment = Base64.decode(json.getString("commitment"), Base64.NO_WRAP),
            epoch = json.getLong("epoch"),
            timestamp = json.getLong("timestamp"),
            tier = json.getInt("tier"),
            active = json.getBoolean("active"),
            groupType = json.getInt("group_type"),
            memberCount = json.getInt("member_count")
        )
    }
}

/** JSON builder for create_group request payload. */
fun buildCreateGroupPayload(
    caller: String,
    groupID: ByteArray,
    commitment: ByteArray,
    proof: ByteArray,
    commitmentForInputs: ByteArray,
    epoch: Long,
    tier: Int
): JSONObject = JSONObject().apply {
    put("caller", caller)
    put("groupID", groupID.toBase64())
    put("commitment", commitment.toBase64())
    put("proof", proof.toBase64())
    put("publicInputs", JSONObject().apply {
        put("commitment", commitmentForInputs.toBase64())
        put("epoch", epoch)
    })
    put("tier", tier)
}

/**
 * JSON builder for update_commitment request payload (#59 UpdateCircuit fix).
 *
 * The contract binds `cNew` cryptographically inside the proof, so the relayer
 * no longer accepts a client-supplied `newCommitment`/`newEpoch`. Only the
 * UpdateCircuit public inputs are sent; the contract derives the new epoch
 * as `epoch_old + 1` in circuit.
 */
fun buildUpdateCommitmentPayload(
    groupID: ByteArray,
    proof: ByteArray,
    cOld: ByteArray,
    epochOld: Long,
    cNew: ByteArray
): JSONObject = JSONObject().apply {
    put("groupID", groupID.toBase64())
    put("proof", proof.toBase64())
    put("publicInputs", JSONObject().apply {
        put("c_old", cOld.toBase64())
        put("epoch_old", epochOld)
        put("c_new", cNew.toBase64())
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

/**
 * JSON builder for `create_group_v2` — the governance-aware creation
 * entrypoint. Supports Anarchy (`groupType = 0`), 1v1 (`groupType = 1`), and
 * Democracy (`groupType = 2`). Oligarchy uses `buildCreateOligarchyGroupPayload`
 * because it also seeds the admin root. JSON keys are snake_case to match
 * the relayer's V2 payload schema.
 */
fun buildCreateGroupV2Payload(
    caller: String,
    groupID: ByteArray,
    commitment: ByteArray,
    tier: Int,
    groupType: Int,
    memberCount: Int,
    proof: ByteArray,
    commitmentForInputs: ByteArray,
    epoch: Long
): JSONObject = JSONObject().apply {
    put("caller", caller)
    put("group_id", groupID.toBase64())
    put("commitment", commitment.toBase64())
    put("tier", tier)
    put("group_type", groupType)
    put("member_count", memberCount)
    put("proof", proof.toBase64())
    put("public_inputs", JSONObject().apply {
        put("commitment", commitmentForInputs.toBase64())
        put("epoch", epoch)
    })
}

/**
 * JSON builder for `create_oligarchy_group`. `adminRoot` is the Poseidon-
 * hashed admin_commitment (see SEPCommitmentBuilder.computeAdminCommitment)
 * and is pinned at creation; later admin rotations are ceremony-gated.
 */
fun buildCreateOligarchyGroupPayload(
    caller: String,
    groupID: ByteArray,
    commitment: ByteArray,
    tier: Int,
    memberCount: Int,
    adminRoot: ByteArray,
    proof: ByteArray,
    commitmentForInputs: ByteArray,
    epoch: Long
): JSONObject = JSONObject().apply {
    put("caller", caller)
    put("group_id", groupID.toBase64())
    put("commitment", commitment.toBase64())
    put("tier", tier)
    put("member_count", memberCount)
    put("admin_root", adminRoot.toBase64())
    put("proof", proof.toBase64())
    put("public_inputs", JSONObject().apply {
        put("commitment", commitmentForInputs.toBase64())
        put("epoch", epoch)
    })
}

private fun ByteArray.toBase64(): String = Base64.encodeToString(this, Base64.NO_WRAP)
