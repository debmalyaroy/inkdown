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

    @Override
    protected void onCreate(android.os.Bundle state) {
        super.onCreate(state);
        useFastestRefreshRate();
        received(getIntent());
    }

    /**
     * Something another app shared with Inkdown ("Share" and then Inkdown): a file is
     * opened like any other, and text becomes a new note.
     */
    private void received(Intent intent) {
        if (intent == null || !Intent.ACTION_SEND.equals(intent.getAction())) {
            return;
        }
        Uri stream = intent.getParcelableExtra(Intent.EXTRA_STREAM);
        if (stream != null) {
            synchronized (lock) {
                opened.add(stream.toString());
            }
            return;
        }
        CharSequence text = intent.getCharSequenceExtra(Intent.EXTRA_TEXT);
        if (text == null || text.length() == 0) {
            return;
        }
        try {
            String subject = intent.getStringExtra(Intent.EXTRA_SUBJECT);
            String name = subject == null ? "" : subject.replaceAll("[^\\p{L}\\p{N} ._-]", " ").trim();
            if (name.length() > 60) {
                name = name.substring(0, 60).trim();
            }
            if (name.length() == 0) {
                name = "Shared text";
            }
            java.io.File dir = new java.io.File(getFilesDir(), "shared-in");
            dir.mkdirs();
            java.io.File file = new java.io.File(dir, name + ".md");
            java.io.FileOutputStream out = new java.io.FileOutputStream(file);
            String body = (subject != null && subject.trim().length() > 0 ? "# " + subject.trim() + "\n\n" : "") + text + "\n";
            out.write(body.getBytes("UTF-8"));
            out.close();
            // read back by the native side through the same door as any other document
            Uri uri = ShareProvider.offer(file, file.getName());
            synchronized (lock) {
                opened.add(uri.toString());
            }
        } catch (Exception e) {
            // the note could not be written: nothing arrives
        }
    }

    @Override
    public void onWindowFocusChanged(boolean focused) {
        super.onWindowFocusChanged(focused);
        if (focused) {
            useFastestRefreshRate();
        }
    }

    /**
     * Asks for the highest refresh rate the screen has at its present resolution. Without
     * this, phones with a 90 or 120 Hz screen hold an app that draws its own surface at
     * 60 frames a second, and scrolling looks slow next to every other app.
     */
    private void useFastestRefreshRate() {
        try {
            android.view.Display display = getWindowManager().getDefaultDisplay();
            android.view.Display.Mode now = display.getMode();
            android.view.Display.Mode best = now;
            for (android.view.Display.Mode m : display.getSupportedModes()) {
                if (m.getPhysicalWidth() == now.getPhysicalWidth() && m.getPhysicalHeight() == now.getPhysicalHeight()
                        && m.getRefreshRate() > best.getRefreshRate() + 0.5f) {
                    best = m;
                }
            }
            android.view.WindowManager.LayoutParams lp = getWindow().getAttributes();
            lp.preferredDisplayModeId = best.getModeId();
            lp.preferredRefreshRate = best.getRefreshRate();
            getWindow().setAttributes(lp);
            if (android.os.Build.VERSION.SDK_INT >= 35) {
                // screens that vary their rate also want to be told that this window moves
                // (the method is newer than the SDK this file is compiled against)
                getWindow().getClass().getMethod("setFrameRateBoostOnTouchEnabled", boolean.class).invoke(getWindow(), true);
            }
        } catch (Throwable e) {
            // an unusual display: the default rate is fine
        }
    }

    /** "Open with Inkdown" while Inkdown is already running. */
    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        received(intent);
        Uri uri = intent.getData();
        if (uri != null) {
            synchronized (lock) {
                opened.add(uri.toString());
            }
        }
    }

    /**
     * Back, when there is nothing left to go back from inside the app: put the app in the
     * background, like the Home button. The documents stay open.
     */
    public void leaveApp() {
        runOnUiThread(new Runnable() {
            @Override
            public void run() {
                moveTaskToBack(true);
            }
        });
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

    /**
     * Offers a file to other apps: the system's share sheet opens, and the app the user
     * picks is allowed to read this one file (see ShareProvider).
     */
    public void shareFile(final String path, final String name, final String mime) {
        runOnUiThread(new Runnable() {
            @Override
            public void run() {
                try {
                    Uri uri = ShareProvider.offer(new java.io.File(path), name);
                    Intent send = new Intent(Intent.ACTION_SEND);
                    send.setType(mime);
                    send.putExtra(Intent.EXTRA_STREAM, uri);
                    send.putExtra(Intent.EXTRA_SUBJECT, name);
                    send.putExtra(Intent.EXTRA_TITLE, name);
                    // the chooser passes the permission on through the clip data
                    send.setClipData(ClipData.newRawUri(name, uri));
                    send.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION);
                    Intent chooser = Intent.createChooser(send, name);
                    chooser.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION);
                    startActivity(chooser);
                } catch (Exception e) {
                    // no app on this device can receive the file
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

    /** Open web connections of documents read from a server, by id. */
    private final java.util.HashMap<Integer, Object[]> http = new java.util.HashMap<Integer, Object[]>();
    private int httpNext = 1;

    /**
     * Asks for a web address (documents opened from a server) and returns an id for the
     * answer once its headers are in. Called from worker threads of the native side, never
     * from the UI thread. httpHead gives status and headers, httpRead the body piece by
     * piece as it arrives (a server may keep sending for as long as it likes), httpClose ends it.
     */
    public int httpOpen(String url, long from, String etag, String modified) {
        java.net.HttpURLConnection c = null;
        String head;
        java.io.InputStream in = null;
        try {
            c = (java.net.HttpURLConnection) new java.net.URL(url).openConnection();
            c.setConnectTimeout(15000);
            // no read timeout: a stream may be quiet for a long time
            c.setReadTimeout(0);
            c.setInstanceFollowRedirects(true);
            c.setRequestProperty("User-Agent", "Inkdown");
            c.setRequestProperty("Accept", "text/*, application/json, */*;q=0.8");
            // the native side counts bytes of the document itself
            c.setRequestProperty("Accept-Encoding", "identity");
            if (from > 0) {
                c.setRequestProperty("Range", "bytes=" + from + "-");
                String validator = etag.length() > 0 ? etag : modified;
                if (validator.length() > 0) {
                    c.setRequestProperty("If-Range", validator);
                }
            } else {
                if (etag.length() > 0) {
                    c.setRequestProperty("If-None-Match", etag);
                }
                if (modified.length() > 0) {
                    c.setRequestProperty("If-Modified-Since", modified);
                }
            }
            int status = c.getResponseCode();
            head = status + "\n" + line(c.getHeaderField("ETag")) + "\n" + line(c.getHeaderField("Last-Modified")) + "\n" + line(c.getHeaderField("Content-Range"));
            if (status >= 200 && status < 300) {
                in = c.getInputStream();
            }
        } catch (Exception e) {
            head = "0\n" + line(e.toString()) + "\n\n";
        }
        synchronized (http) {
            int id = httpNext++;
            http.put(id, new Object[] { head, c, in });
            return id;
        }
    }

    public String httpHead(int id) {
        synchronized (http) {
            Object[] entry = http.get(id);
            return entry == null ? "0\nclosed\n\n" : (String) entry[0];
        }
    }

    /** The next piece of the body: empty at its end, null when the connection failed. */
    public byte[] httpRead(int id, int max) {
        java.io.InputStream in;
        synchronized (http) {
            Object[] entry = http.get(id);
            in = entry == null ? null : (java.io.InputStream) entry[2];
        }
        if (in == null) {
            return new byte[0];
        }
        try {
            byte[] buf = new byte[Math.max(1, Math.min(max, 64 * 1024))];
            int n = in.read(buf);
            return n <= 0 ? new byte[0] : java.util.Arrays.copyOf(buf, n);
        } catch (Exception e) {
            return null;
        }
    }

    public void httpClose(int id) {
        Object[] entry;
        synchronized (http) {
            entry = http.remove(id);
        }
        if (entry == null) {
            return;
        }
        try {
            if (entry[2] != null) {
                ((java.io.InputStream) entry[2]).close();
            }
        } catch (Exception e) {
            // closing a connection that is already gone
        }
        if (entry[1] != null) {
            ((java.net.HttpURLConnection) entry[1]).disconnect();
        }
    }

    private static String line(String s) {
        return s == null ? "" : s.replace('\n', ' ').replace('\r', ' ');
    }
}
