// nimrag-sync — keeps an inReach's software, contacts, messages and settings
// in sync on Linux.

#include <QGuiApplication>
#include <QIcon>
#include <QQmlApplicationEngine>
#include <QQmlContext>
#include <QQuickStyle>
#include <QUrl>

#include <cstring>

#include "backend.h"
#include "nimrag.h"
#include "theme.h"
#include "translator.h"

int main(int argc, char *argv[]) {
    // Every command but `open` is the Rust core's command line, which needs no
    // display; the window opens without arguments or with `open`.
    if (argc > 1 && std::strcmp(argv[1], "open") != 0)
        return nimrag_cli_main(argc, argv);

    QGuiApplication app(argc, argv);
    app.setApplicationName("nimrag-sync");
    app.setApplicationVersion("0.1.0");

    // Matches the window to nimrag-sync.desktop, so the compositor (Wayland
    // app_id) and the launcher pick up the installed icon.
    app.setDesktopFileName("nimrag-sync");
    app.setWindowIcon(QIcon::fromTheme("nimrag-sync"));

    QQuickStyle::setStyle("Material");

    // Installed before anything is translated, and every qsTr() binding is
    // re-evaluated when the Omarchy language changes.
    Translator translator;
    app.installTranslator(&translator);

    Theme theme;
    Backend backend;

    QQmlApplicationEngine engine;
    QObject::connect(&translator, &Translator::changed, &engine, &QQmlEngine::retranslate);
    engine.rootContext()->setContextProperty("translator", &translator);
    engine.rootContext()->setContextProperty("theme", &theme);
    engine.rootContext()->setContextProperty("backend", &backend);
    engine.load(QUrl("qrc:/Main.qml"));
    if (engine.rootObjects().isEmpty())
        return 1;

    return app.exec();
}
