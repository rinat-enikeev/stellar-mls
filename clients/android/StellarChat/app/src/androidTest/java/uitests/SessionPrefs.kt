package uitests

import android.content.Context
import androidx.test.platform.app.InstrumentationRegistry

/**
 * Minimal pre-test reset.
 *
 * The previous suite (`Prefs.kt`) wiped *every* SharedPreferences bucket and
 * forced empty strings into `relay_urls`/`endpoint`/`contract_id`/`relayer_url`,
 * which severed all real networking. The user wants tests to run with the
 * **production network** intact (Nostr relays, Soroban contract, relayer),
 * so we no longer touch those buckets.
 *
 * What we still need to clear is the onboarding-seen flag — otherwise on a
 * device that has already run the app once the welcome bottom sheet won't
 * render and the journey can't start. The same applies to the in-chat first-
 * group welcome bubble.
 *
 * We do NOT wipe the Room database (`stellar_chat.db`) — the device may be
 * the user's daily driver and contain real groups. The journey creates
 * groups with unique names ("Anarchy Crew QA", …) and deletes them at the
 * end, leaving any pre-existing state untouched.
 */
fun clearOnboardingFlags() {
    val ctx = InstrumentationRegistry.getInstrumentation().targetContext
    ctx.getSharedPreferences("stellar_chat", Context.MODE_PRIVATE)
        .edit()
        .remove("has_seen_onboarding")
        .remove("has_seen_first_group_welcome")
        .commit()
}
