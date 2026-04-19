package chat.onym.android.contacts

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.net.Uri
import android.provider.ContactsContract
import android.telephony.TelephonyManager
import androidx.core.content.ContextCompat
import java.util.Locale

data class PhoneContact(
    val id: String,        // stable-ish: phone (normalized) — doubles as primary key in store
    val displayName: String,
    val phone: String      // normalized E.164
)

enum class ContactsPermission { NOT_DETERMINED, DENIED, AUTHORIZED }

object SystemContactsImporter {
    fun permission(context: Context): ContactsPermission {
        return when (
            ContextCompat.checkSelfPermission(context, Manifest.permission.READ_CONTACTS)
        ) {
            PackageManager.PERMISSION_GRANTED -> ContactsPermission.AUTHORIZED
            else -> ContactsPermission.NOT_DETERMINED
        }
    }

    /** Read all phone-bearing contacts from the system phonebook.
     *  Returns de-duplicated list keyed by normalized E.164 phone. */
    fun fetchAll(context: Context): List<PhoneContact> {
        val defaultRegion = deviceRegion(context)
        val seen = LinkedHashMap<String, PhoneContact>()

        val uri: Uri = ContactsContract.CommonDataKinds.Phone.CONTENT_URI
        val projection = arrayOf(
            ContactsContract.CommonDataKinds.Phone.DISPLAY_NAME,
            ContactsContract.CommonDataKinds.Phone.NUMBER,
            ContactsContract.CommonDataKinds.Phone.NORMALIZED_NUMBER
        )

        context.contentResolver.query(uri, projection, null, null, null)?.use { cursor ->
            val nameIdx = cursor.getColumnIndex(ContactsContract.CommonDataKinds.Phone.DISPLAY_NAME)
            val numIdx = cursor.getColumnIndex(ContactsContract.CommonDataKinds.Phone.NUMBER)
            val normIdx = cursor.getColumnIndex(ContactsContract.CommonDataKinds.Phone.NORMALIZED_NUMBER)
            while (cursor.moveToNext()) {
                val name = if (nameIdx >= 0) cursor.getString(nameIdx) else null
                val rawNum = if (numIdx >= 0) cursor.getString(numIdx) else null
                val normNum = if (normIdx >= 0) cursor.getString(normIdx) else null
                val number = (normNum ?: rawNum) ?: continue
                val e164 = normalize(number, defaultRegion) ?: continue
                if (seen.containsKey(e164)) continue
                seen[e164] = PhoneContact(
                    id = e164,
                    displayName = name?.trim()?.ifEmpty { e164 } ?: e164,
                    phone = e164
                )
            }
        }

        return seen.values.sortedBy { it.displayName.lowercase(Locale.getDefault()) }
    }

    /** Normalize a phone string to E.164. Very light-weight: keeps only digits,
     *  prefixes the device's calling code if the input lacks a `+`. */
    fun normalize(input: String, defaultRegion: String?): String? {
        val trimmed = input.trim()
        if (trimmed.isEmpty()) return null
        val hasPlus = trimmed.startsWith("+")
        val digits = trimmed.filter { it.isDigit() }
        if (digits.isEmpty()) return null
        if (hasPlus) return "+$digits"
        val cc = defaultRegion?.let { callingCodeFor(it) } ?: return null
        val stripped = digits.trimStart('0')
        if (stripped.isEmpty()) return null
        // Avoid double-prefixing if the number already begins with the calling code
        val withCountry = if (stripped.startsWith(cc)) stripped else cc + stripped
        return "+$withCountry"
    }

    private fun deviceRegion(context: Context): String? {
        val tm = context.getSystemService(Context.TELEPHONY_SERVICE) as? TelephonyManager
        val iso = tm?.networkCountryIso ?: tm?.simCountryIso
        return iso?.uppercase(Locale.ROOT)?.takeIf { it.isNotEmpty() }
            ?: Locale.getDefault().country.takeIf { it.isNotEmpty() }
    }

    /** Minimal country ISO → calling code table. Covers common regions; extend as needed. */
    private fun callingCodeFor(regionISO: String): String? = when (regionISO.uppercase(Locale.ROOT)) {
        "US", "CA" -> "1"
        "GB" -> "44"
        "DE" -> "49"
        "FR" -> "33"
        "IT" -> "39"
        "ES" -> "34"
        "NL" -> "31"
        "RU" -> "7"
        "KZ" -> "7"
        "UA" -> "380"
        "BY" -> "375"
        "PL" -> "48"
        "TR" -> "90"
        "IN" -> "91"
        "CN" -> "86"
        "JP" -> "81"
        "KR" -> "82"
        "BR" -> "55"
        "MX" -> "52"
        "AR" -> "54"
        "AU" -> "61"
        "NZ" -> "64"
        "ZA" -> "27"
        "EG" -> "20"
        "AE" -> "971"
        "SA" -> "966"
        "IL" -> "972"
        "ID" -> "62"
        "MY" -> "60"
        "SG" -> "65"
        "PH" -> "63"
        "TH" -> "66"
        "VN" -> "84"
        "PK" -> "92"
        "BD" -> "880"
        "NG" -> "234"
        "KE" -> "254"
        "SE" -> "46"
        "NO" -> "47"
        "FI" -> "358"
        "DK" -> "45"
        "CH" -> "41"
        "AT" -> "43"
        "BE" -> "32"
        "IE" -> "353"
        "PT" -> "351"
        "GR" -> "30"
        "CZ" -> "420"
        "HU" -> "36"
        "RO" -> "40"
        else -> null
    }
}
