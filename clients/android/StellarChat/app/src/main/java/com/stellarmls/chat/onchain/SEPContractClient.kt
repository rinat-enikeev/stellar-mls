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
        caller: String,
        groupID: ByteArray,
        commitment: ByteArray,
        proof: ByteArray,
        publicInputsCommitment: ByteArray,
        epoch: Long,
        tier: Int
    ): SEPSubmissionResponse {
        val payload = buildCreateGroupPayload(
            caller, groupID, commitment, proof, publicInputsCommitment, epoch, tier
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

    fun deactivateGroup(
        groupID: ByteArray,
        proof: ByteArray,
        commitment: ByteArray,
        epoch: Long
    ): SEPSubmissionResponse {
        val payload = buildDeactivateGroupPayload(groupID, proof, commitment, epoch)
        val json = transport.invoke(contractID, "deactivate_group", payload)
        return SEPSubmissionResponse.fromJson(json)
    }

    fun getState(groupID: ByteArray): SEPCommitmentEntry {
        val payload = buildGetStatePayload(groupID)
        val json = transport.invoke(contractID, "get_state", payload)
        return SEPCommitmentEntry.fromJson(json)
    }
}

/**
 * Relayer transport that routes contract invocations through a fee-paying relayer.
 * The relayer wraps the payload in a Stellar transaction signed with its own keypair,
 * preventing identity leakage through the transaction signer address (SEP-XXXX §4.1).
 */
/**
 * Relayer transport that routes contract invocations through a fee-paying relayer.
 * The relayer wraps the payload in a Stellar transaction signed with its own keypair,
 * preventing identity leakage through the transaction signer address (SEP-XXXX §4.1).
 *
 * @param pinnedCertificateHashes SHA-256 hashes of the relayer's TLS certificate public keys
 *        (base64-encoded). When non-empty, connections to relayers whose certificate doesn't
 *        match are rejected, preventing MITM interception of proofs (H-14).
 */
class OkHttpRelayerTransport(
    private val relayerURL: String,
    private val authToken: String? = null,
    pinnedCertificateHashes: List<String> = emptyList(),
    private val client: OkHttpClient = buildClient(relayerURL, pinnedCertificateHashes)
) : SEPContractTransport {

    companion object {
        private fun buildClient(relayerURL: String, pins: List<String>): OkHttpClient {
            if (pins.isEmpty()) return OkHttpClient()
            val host = java.net.URL(relayerURL).host
            val pinner = okhttp3.CertificatePinner.Builder().apply {
                for (hash in pins) add(host, "sha256/$hash")
            }.build()
            return OkHttpClient.Builder().certificatePinner(pinner).build()
        }
    }

    private val jsonMediaType = "application/json; charset=utf-8".toMediaType()

    override fun invoke(contractID: String, function: String, payload: JSONObject): JSONObject {
        val invocation = JSONObject().apply {
            put("contractID", contractID)
            put("function", function)
            put("payload", payload)
        }

        val builder = Request.Builder()
            .url(relayerURL)
            .post(invocation.toString().toRequestBody(jsonMediaType))

        if (!authToken.isNullOrBlank()) {
            builder.addHeader("Authorization", "Bearer $authToken")
        }

        val response = client.newCall(builder.build()).execute()
        val responseBody = response.body?.string() ?: ""

        if (!response.isSuccessful) {
            throw IOException("Relayer call failed: ${response.code} $responseBody")
        }

        return JSONObject(responseBody)
    }
}
