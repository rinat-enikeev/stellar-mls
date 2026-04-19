package chat.onym.android.ui.components

import android.content.Context
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color

/**
 * Six-stop accent palette used by the Create Group flow and (eventually) per-group theming.
 * Gradients match the iOS SwiftUI `GroupAccent` enum.
 */
enum class GroupAccent(private val stops: List<Color>, val shadow: Color) {
    ORANGE(listOf(Color(0xFFFF9500), Color(0xFFFF2D55)), Color(0xFFFF9500)),
    BLUE(listOf(Color(0xFF0A84FF), Color(0xFF5AC8FA)), Color(0xFF0A84FF)),
    GREEN(listOf(Color(0xFF30D158), Color(0xFF34C759)), Color(0xFF30D158)),
    PURPLE(listOf(Color(0xFFAF52DE), Color(0xFFBF5AF2)), Color(0xFFAF52DE)),
    PINK(listOf(Color(0xFFFF375F), Color(0xFFFF2D55)), Color(0xFFFF375F)),
    YELLOW(listOf(Color(0xFFFFD60A), Color(0xFFFF9F0A)), Color(0xFFFFD60A));

    /** Diagonal linear gradient suitable for Brush-filled shapes. */
    fun brush(): Brush = Brush.linearGradient(colors = stops)

    val primary: Color get() = stops.first()

    companion object {
        /** Stable assignment from an arbitrary seed string (e.g., a group ID). */
        fun fromSeed(seed: String): GroupAccent {
            var hash = 5381L
            for (ch in seed.toByteArray(Charsets.UTF_8)) {
                hash = (hash * 33) + ch.toLong()
            }
            val all = entries
            val idx = ((hash % all.size) + all.size) % all.size
            return all[idx.toInt()]
        }
    }
}

/** Per-group accent persistence via SharedPreferences, keyed by group ID. */
object GroupAccentStore {
    private const val PREFS = "chat.onym.groupAccent"

    fun set(context: Context, accent: GroupAccent, groupID: String) {
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .edit()
            .putString(groupID, accent.name)
            .apply()
    }

    fun get(context: Context, groupID: String): GroupAccent {
        val raw = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .getString(groupID, null) ?: return GroupAccent.fromSeed(groupID)
        return runCatching { GroupAccent.valueOf(raw) }.getOrDefault(GroupAccent.fromSeed(groupID))
    }
}
