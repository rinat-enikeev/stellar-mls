package chat.onym.android.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

/**
 * Rounded-square colored icon used throughout Settings (mirrors iOS SettingsIconBox).
 * Default size of 30dp matches UIKit's standard list-row icon metric.
 */
@Composable
fun SettingsIconBox(
    icon: ImageVector,
    background: Color,
    modifier: Modifier = Modifier,
    size: Dp = 30.dp
) {
    Box(
        modifier = modifier
            .size(size)
            .background(background, RoundedCornerShape(if (size >= 30.dp) 7.dp else 6.dp)),
        contentAlignment = Alignment.Center
    ) {
        Icon(
            icon,
            contentDescription = null,
            tint = Color.White,
            modifier = Modifier.size(size * 0.55f)
        )
    }
}

object SettingsIconColors {
    val Orange = Color(0xFFFF9500)
    val Purple = Color(0xFFAF52DE)
    val Cyan = Color(0xFF59C6F9)
    val Green = Color(0xFF34C759)
    val Yellow = Color(0xFFFFCC00)
    val Gray = Color(0xFF8E8E93)
    val Blue = Color(0xFF007AFF)
    val Pink = Color(0xFFFF2D55)
    val Red = Color(0xFFFF3B30)
}
