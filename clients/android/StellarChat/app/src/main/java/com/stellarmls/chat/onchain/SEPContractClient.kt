package com.stellarmls.chat.onchain

import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import org.json.JSONObject
import java.io.IOException

/** HTTP transport interface for Soroban contract invocations. */
interface SEPContractTransport {
    fun invoke(contractID: String, function: String, payload: JSONObject): JSONObject
}

/** OkHttp-based transport that POSTs JSON to a Soroban RPC endpoint. */
class OkHttpSEPContractTransport(
    private val endpoint: String,
    private val client: OkHttpClient = OkHttpClient()
) : SEPContractTransport {

    private val jsonMediaType = "application/json; charset=utf-8".toMediaType()

    override fun invoke(contractID: String, function: String, payload: JSONObject): JSONObject {
        val invocation = JSONObject().apply {
            put("contractID", contractID)
            put("function", function)
            put("payload", payload)
        }

        val body = invocation.toString().toRequestBody(jsonMediaType)
        val request = Request.Builder()
            .url(endpoint)
            .post(body)
            .build()

        val response = client.newCall(request).execute()
        val responseBody = response.body?.string() ?: ""

        if (!response.isSuccessful) {
            throw IOException("Contract call failed: ${response.code} $responseBody")
        }

        return JSONObject(responseBody)
    }
}

/** High-level contract client wrapping the transport with typed methods. */
class SEPContractClient(
    val contractID: String,
    private val transport: SEPContractTransport
) {
    fun createGroup(
        groupID: ByteArray,
        commitment: ByteArray,
        proof: ByteArray,
        publicInputsCommitment: ByteArray,
        epoch: Long,
        tier: Int
    ): SEPSubmissionResponse {
        val payload = buildCreateGroupPayload(
            groupID, commitment, proof, publicInputsCommitment, epoch, tier
        )
        val json = transport.invoke(contractID, "create_group", payload)
        return SEPSubmissionResponse.fromJson(json)
    }

    fun updateCommitment(
        groupID: ByteArray,
        newCommitment: ByteArray,
        newEpoch: Long,
        proof: ByteArray,
        oldCommitment: ByteArray,
        oldEpoch: Long
    ): SEPSubmissionResponse {
        val payload = buildUpdateCommitmentPayload(
            groupID, newCommitment, newEpoch, proof, oldCommitment, oldEpoch
        )
        val json = transport.invoke(contractID, "update_commitment", payload)
        return SEPSubmissionResponse.fromJson(json)
    }

    fun verifyMembership(
        groupID: ByteArray,
        proof: ByteArray,
        commitment: ByteArray,
        epoch: Long
    ): SEPVerifyMembershipResponse {
        val payload = buildVerifyMembershipPayload(groupID, proof, commitment, epoch)
        val json = transport.invoke(contractID, "verify_membership", payload)
        return SEPVerifyMembershipResponse.fromJson(json)
    }

    fun getState(groupID: ByteArray): SEPCommitmentEntry {
        val payload = buildGetStatePayload(groupID)
        val json = transport.invoke(contractID, "get_state", payload)
        return SEPCommitmentEntry.fromJson(json)
    }
}
