package app.inkdown.editor;

import android.content.ContentProvider;
import android.content.ContentValues;
import android.database.Cursor;
import android.database.MatrixCursor;
import android.net.Uri;
import android.os.ParcelFileDescriptor;
import android.provider.OpenableColumns;

import java.io.File;
import java.io.FileNotFoundException;
import java.util.HashMap;

/**
 * Lets another app read a file that Inkdown shares with it ("Share" in the export sheet).
 *
 * Android does not allow handing a path of the app's private storage to another app; the
 * file has to be offered through a content provider, and the receiving app is given
 * permission to read that one address. Only files that were explicitly offered with
 * {@link #offer} can be read, by their name, and only for reading.
 */
public class ShareProvider extends ContentProvider {
    public static final String AUTHORITY = "app.inkdown.editor.share";

    private static final HashMap<String, File> offered = new HashMap<String, File>();

    /** Makes a file readable under its name and returns the address to share. */
    public static Uri offer(File file, String name) {
        synchronized (offered) {
            offered.put(name, file);
        }
        return new Uri.Builder().scheme("content").authority(AUTHORITY).appendPath(name).build();
    }

    private static File find(Uri uri) {
        String name = uri.getLastPathSegment();
        synchronized (offered) {
            return name == null ? null : offered.get(name);
        }
    }

    /** The type of a file, from the end of its name. */
    public static String typeOf(String name) {
        String n = name.toLowerCase();
        if (n.endsWith(".pdf")) return "application/pdf";
        if (n.endsWith(".html") || n.endsWith(".htm")) return "text/html";
        if (n.endsWith(".docx")) return "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
        if (n.endsWith(".rtf")) return "application/rtf";
        if (n.endsWith(".md") || n.endsWith(".markdown")) return "text/markdown";
        if (n.endsWith(".json")) return "application/json";
        if (n.endsWith(".csv")) return "text/csv";
        if (n.endsWith(".png")) return "image/png";
        if (n.endsWith(".jpg") || n.endsWith(".jpeg")) return "image/jpeg";
        if (n.endsWith(".svg")) return "image/svg+xml";
        if (n.endsWith(".avro")) return "application/octet-stream";
        return "text/plain";
    }

    @Override
    public boolean onCreate() {
        return true;
    }

    @Override
    public String getType(Uri uri) {
        String name = uri.getLastPathSegment();
        return name == null ? null : typeOf(name);
    }

    @Override
    public Cursor query(Uri uri, String[] projection, String selection, String[] args, String order) {
        File file = find(uri);
        String[] columns = projection != null ? projection : new String[] { OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE };
        MatrixCursor cursor = new MatrixCursor(columns, 1);
        if (file != null) {
            Object[] row = new Object[columns.length];
            for (int i = 0; i < columns.length; i++) {
                if (OpenableColumns.DISPLAY_NAME.equals(columns[i])) {
                    row[i] = uri.getLastPathSegment();
                } else if (OpenableColumns.SIZE.equals(columns[i])) {
                    row[i] = file.length();
                }
            }
            cursor.addRow(row);
        }
        return cursor;
    }

    @Override
    public ParcelFileDescriptor openFile(Uri uri, String mode) throws FileNotFoundException {
        File file = find(uri);
        if (file == null || !file.isFile() || !"r".equals(mode)) {
            throw new FileNotFoundException("Not shared: " + uri);
        }
        return ParcelFileDescriptor.open(file, ParcelFileDescriptor.MODE_READ_ONLY);
    }

    @Override
    public Uri insert(Uri uri, ContentValues values) {
        return null;
    }

    @Override
    public int update(Uri uri, ContentValues values, String selection, String[] args) {
        return 0;
    }

    @Override
    public int delete(Uri uri, String selection, String[] args) {
        return 0;
    }
}
