package io.github.keepingrunning.leaftrial;

import android.app.Activity;
import android.content.Intent;
import android.database.Cursor;
import android.net.Uri;
import android.os.Bundle;
import android.provider.DocumentsContract;
import android.view.View;
import android.widget.Button;
import android.widget.LinearLayout;
import android.widget.TextView;

import java.io.File;
import java.io.IOException;
import java.io.InputStream;
import java.nio.file.Files;
import java.nio.file.StandardCopyOption;
import java.util.Locale;
import java.util.UUID;

public final class LibraryActivity extends Activity {
    private LinearLayout layout;
    private TextView status;
    private View[] controls;
    private static final int MAIN = 1, SPECIAL = 2;

    @Override public void onCreate(Bundle state) {
        super.onCreate(state);
        layout = new LinearLayout(this);
        layout.setPadding(36, 64, 36, 36);
        layout.setOrientation(LinearLayout.VERTICAL);
        setContentView(layout);
        controls = new View[]{addGame(MAIN, "WHITE ALBUM2"), addGame(SPECIAL, "Special Contents")};
        status = new TextView(this);
        layout.addView(status);
        status.setText("首次使用请选择完整解压后的游戏目录。资源会复制到本应用，之后可离线启动。");
    }

    private File directory(int game) { return new File(getFilesDir(), game == MAIN ? "Main" : "Special"); }
    private Button addGame(int game, String name) {
        Button button = new Button(this);
        layout.addView(button);
        button.setText(name);
        button.setOnClickListener(v -> {
            if (directory(game).isDirectory()) {
                Intent player = new Intent(this, PlayerActivity.class);
                player.putExtra("directory", directory(game).getAbsolutePath());
                player.putExtra("mode", game == MAIN ? "main" : "special");
                startActivity(player);
            } else {
                startActivityForResult(new Intent(Intent.ACTION_OPEN_DOCUMENT_TREE), game);
            }
        });
        return button;
    }

    @Override protected void onActivityResult(int game, int result, Intent data) {
        super.onActivityResult(game, result, data);
        if ((game != MAIN && game != SPECIAL) || result != RESULT_OK || data == null || data.getData() == null) return;
        Uri tree = data.getData();
        for (View v : controls) v.setEnabled(false);
        status.setText("正在导入游戏资源…");
        new Thread(() -> {
            File staging = new File(getFilesDir(), ".import-" + UUID.randomUUID());
            try {
                Files.createDirectory(staging.toPath());
                copyDirectory(tree, DocumentsContract.getTreeDocumentId(tree), staging, 0);
                if (!hasArchive(staging, game == MAIN ? "ck-gal.pak" : "script_cn.pak"))
                    throw new IOException("所选目录缺少此模式的剧本包，请选择直接包含 PAK 文件的目录。");
                Files.move(staging.toPath(), directory(game).toPath(), StandardCopyOption.ATOMIC_MOVE);
                showStatus("导入完成，点击游戏即可启动。");
            } catch (Exception error) {
                clear(staging);
                showStatus("导入失败：" + error.getMessage());
            }
        }, "leaf-import").start();
    }

    private void showStatus(String message) {
        runOnUiThread(() -> {
            if (isDestroyed()) return;
            status.setText(message);
            for (View v : controls) v.setEnabled(true);
        });
    }

    private static boolean hasArchive(File dir, String name) {
        File[] files = dir.listFiles();
        if (files != null) for (File file : files)
            if (file.isFile() && file.getName().equalsIgnoreCase(name)) return true;
        return false;
    }

    private void copyDirectory(Uri tree, String parent, File dest, int depth) throws IOException {
        if (depth > 32) throw new IOException("目录嵌套过深。");
        Uri children = DocumentsContract.buildChildDocumentsUriUsingTree(tree, parent);
        String[] columns = {DocumentsContract.Document.COLUMN_DOCUMENT_ID, DocumentsContract.Document.COLUMN_DISPLAY_NAME,
                            DocumentsContract.Document.COLUMN_MIME_TYPE};
        try (Cursor cursor = getContentResolver().query(children, columns, null, null, null)) {
            if (cursor == null) throw new IOException("无法读取目录。");
            while (cursor.moveToNext()) {
                String id = cursor.getString(0), name = cursor.getString(1);
                if (name == null || name.isEmpty() || name.contains("/") || name.equals(".") || name.equals(".."))
                    throw new IOException("无效的文件名。");
                if (name.equals(".shinki")) continue;
                File output = new File(dest, name);
                if (DocumentsContract.Document.MIME_TYPE_DIR.equals(cursor.getString(2))) {
                    // The engine reads primary archives and the IC subdirectory.
                    if (depth != 0 || !name.equalsIgnoreCase("IC")) continue;
                    Files.createDirectory(output.toPath());
                    copyDirectory(tree, id, output, depth + 1);
                } else if (name.toLowerCase(Locale.ROOT).endsWith(".pak")) {
                    runOnUiThread(() -> status.setText("正在复制 " + name));
                    Uri file = DocumentsContract.buildDocumentUriUsingTree(tree, id);
                    try (InputStream input = getContentResolver().openInputStream(file)) {
                        if (input == null) throw new IOException("无法读取 " + name);
                        Files.copy(input, output.toPath());
                    }
                }
            }
        }
    }

    private static void clear(File file) {
        File[] children = file.listFiles();
        if (children != null) for (File child : children) clear(child);
        file.delete();
    }
}
