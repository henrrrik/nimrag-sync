// nimrag-demo <state.json> [language]: the real window with made-up state,
// the default accent and the language given, without a device or Garmin.
#include <QFile>
#include <QGuiApplication>
#include <QQmlApplicationEngine>
#include <QQmlContext>
#include <QQuickStyle>
#include <QTemporaryDir>

#include "fakebackend.h"
#include "theme.h"
#include "translator.h"

int main(int argc, char *argv[]) {
    if (argc < 2) {
        qWarning("usage: nimrag-demo <state.json> [language]");
        return 1;
    }
    QGuiApplication app(argc, argv);
    app.setDesktopFileName("nimrag-demo");
    QQuickStyle::setStyle("Material");

    // The language comes from a file of our own, so Omarchy's stays as it is.
    QTemporaryDir dir;
    QFile language(dir.filePath("language"));
    if (!language.open(QIODevice::WriteOnly))
        return 1;
    language.write(QByteArray(argc > 2 ? argv[2] : "en") + "\n");
    language.close();
    Translator translator(language.fileName());
    app.installTranslator(&translator);

    // A theme directory that does not exist: the default accent and sharp corners.
    Theme theme(dir.filePath("no-theme"));

    QFile state(QString::fromLocal8Bit(argv[1]));
    if (!state.open(QIODevice::ReadOnly)) {
        qWarning("cannot read %s", argv[1]);
        return 1;
    }
    FakeBackend backend(state.readAll());

    QQmlApplicationEngine engine;
    engine.rootContext()->setContextProperty("translator", &translator);
    engine.rootContext()->setContextProperty("theme", &theme);
    engine.rootContext()->setContextProperty("backend", &backend);
    engine.load(QUrl("qrc:/Main.qml"));
    return engine.rootObjects().isEmpty() ? 1 : app.exec();
}
