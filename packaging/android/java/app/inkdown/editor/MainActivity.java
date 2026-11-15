package app.inkdown.editor;

import android.app.NativeActivity;
import android.content.ClipData;
import android.content.Context;
import android.content.Intent;
import android.net.Uri;
import android.print.PrintAttributes;
import android.print.PrintManager;
import android.webkit.WebView;
import android.webkit.WebViewClient;

import java.util.ArrayList;

/**
 * The Android activity of Inkdown.
 *
 * All of the application is native code (see native/src). This class adds the few things
 * Android only offers to Java: the system file pickers, documents that arrive while the app
 * is already running, and printing. The native side calls the public methods and polls
 * the take...() methods; nothing here calls back into native code.
 */
public class MainActivity extends NativeActivity {
    private static final int REQUEST_OPEN = 41;
    private static final int REQUEST_SAVE = 42;

    private final Object lock = new Object();
    private final ArrayList<String> opened = new ArrayList<String>();
    /** null: nothing yet; "": cancelled; otherwise the chosen document. */
    private String saved;
    /** Kept alive until its page has been handed to the print system. */
    private WebView printView;

    /** "Open with Inkdown" while Inkdown is already running. */
    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        Uri uri = intent.getData();
        if (uri != null) {
            synchronized (lock) {
                opened.add(uri.toString());
            }
        }
    }

    /** Shows the system file picker. The choice arrives through takeOpened(). */
    public void pickFile() {
        runOnUiThread(new Runnable() {
            @Override
            public void run() {
                Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT);
                intent.addCategory(Intent.CATEGORY_OPENABLE);
                intent.setType("*/*");
                intent.putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true);
                intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION
                        | Intent.FLAG_GRANT_WRITE_URI_PERMISSION
                        | Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION);
                try {
                    startActivityForResult(intent, REQUEST_OPEN);
                } catch (Exception e) {
                    // no document provider on this device: nothing to pick from
                }
            }
        });
    }

    /** Shows the system "save as" picker. The choice arrives through takeSaved(). */
    public void pickSave(final String name, final String mime) {
        synchronized (lock) {
            saved = null;
        }
        runOnUiThread(new Runnable() {
            @Override
            public void run() {
                Intent intent = new Intent(Intent.ACTION_CREATE_DOCUMENT);
                intent.addCategory(Intent.CATEGORY_OPENABLE);
                intent.setType(mime);
                intent.putExtra(Intent.EXTRA_TITLE, name);
                intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION
                        | Intent.FLAG_GRANT_WRITE_URI_PERMISSION
                        | Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION);
                try {
                    startActivityForResult(intent, REQUEST_SAVE);
                } catch (Exception e) {
                    synchronized (lock) {
                        saved = "";
                    }
                }
            }
        });
    }

    @Override
    protected void onActivityResult(int request, int result, Intent data) {
        super.onActivityResult(request, result, data);
        boolean ok = result == RESULT_OK && data != null;
        if (request == REQUEST_OPEN && ok) {
            ArrayList<Uri> uris = new ArrayList<Uri>();
            ClipData clip = data.getClipData();
            if (clip != null) {
                for (int i = 0; i < clip.getItemCount(); i++) {
                    uris.add(clip.getItemAt(i).getUri());
                }
            } else if (data.getData() != null) {
                uris.add(data.getData());
            }
            for (Uri uri : uris) {
                keep(uri);
                synchronized (lock) {
                    opened.add(uri.toString());
                }
            }
        } else if (request == REQUEST_SAVE) {
            Uri uri = ok ? data.getData() : null;
            if (uri != null) {
                keep(uri);
            }
            synchronized (lock) {
                saved = uri == null ? "" : uri.toString();
            }
        }
    }

    /** Keeps access to a picked document after a restart, where the provider allows it. */
    private void keep(Uri uri) {
        try {
            getContentResolver().takePersistableUriPermission(uri,
                    Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_WRITE_URI_PERMISSION);
        } catch (Exception e) {
            try {
                getContentResolver().takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION);
            } catch (Exception ignored) {
                // access lasts until the app is closed, which is enough to open the file
            }
        }
    }

    /** The next document to open, or null. */
    public String takeOpened() {
        synchronized (lock) {
            return opened.isEmpty() ? null : opened.remove(0);
        }
    }

    /** The document chosen in the "save as" picker; "" when cancelled; null while it is open. */
    public String takeSaved() {
        synchronized (lock) {
            String s = saved;
            saved = null;
            return s;
        }
    }

    /** Sends an HTML page to the system print dialog, which can also save it as a PDF. */
    public void printHtml(final String html, final String name) {
        runOnUiThread(new Runnable() {
            @Override
            public void run() {
                final WebView view = new WebView(MainActivity.this);
                view.getSettings().setJavaScriptEnabled(false);
                view.setWebViewClient(new WebViewClient() {
                    @Override
                    public void onPageFinished(WebView v, String url) {
                        PrintManager manager = (PrintManager) getSystemService(Context.PRINT_SERVICE);
                        if (manager != null) {
                            manager.print(name, v.createPrintDocumentAdapter(name), new PrintAttributes.Builder().build());
                        }
                        printView = null;
                    }
                });
                printView = view;
                view.loadDataWithBaseURL(null, html, "text/html", "utf-8", null);
            }
        });
    }
}
