MaXMB portable build
====================

Run ps3xmb.exe from this folder. Keep the content folder beside it; settings,
profiles, themes, logs, and the WebView profile are stored there.

The portable folder is independent from the source checkout. Its content,
settings, game database, metadata, themes, and WebView profile live under this
folder and are not read from or written to the development project.

The folder is built for Windows x64 and requires the Microsoft Edge WebView2
Runtime. It does not install MaXMB or bundle that Windows runtime.

The package includes the repository's content folder. Files in other locations
are not copied. Game, music, photo, video, and wallpaper paths that refer to
another drive or user folder must be copied to this PC and selected again in
MaXMB's settings.

To rebuild from the repository, run:

  powershell -ExecutionPolicy Bypass -File tools\build-portable.ps1

The default output is a sibling folder named MaXMB-Portable. Existing content
in that output is preserved on subsequent builds. Pass -OutputPath to choose a
different folder outside the source checkout.
