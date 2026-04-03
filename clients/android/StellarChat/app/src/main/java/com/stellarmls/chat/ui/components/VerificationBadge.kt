package com.stellarmls.chat.ui.components

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import com.stellarmls.chat.onchain.OnChainVerificationResult

@Composable
fun VerificationBadge(result: OnChainVerificationResult) {
    val (text, color) = when (result) {
        is OnChainVerificationResult.Verified ->
            "\u2713 Verified on-chain" to Color(0xFF4CAF50)
        is OnChainVerificationResult.NotPublished ->
            "\u25CB Not yet published on-chain" to MaterialTheme.colorScheme.onSurfaceVariant
        is OnChainVerificationResult.EpochMismatch ->
            "\u26A0 Epoch mismatch: local ${result.local}, on-chain ${result.onChain}" to Color(0xFFFF9800)
        is OnChainVerificationResult.CommitmentMismatch ->
            "\u2717 Commitment mismatch" to MaterialTheme.colorScheme.error
        is OnChainVerificationResult.Inactive ->
            "\u2717 Group deactivated" to MaterialTheme.colorScheme.error
        is OnChainVerificationResult.Error ->
            "? Could not verify" to MaterialTheme.colorScheme.onSurfaceVariant
    }
    Text(text, style = MaterialTheme.typography.labelSmall, color = color)
}
