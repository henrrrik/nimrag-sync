QT += core gui qml quick quickcontrols2 dbus concurrent

CONFIG += c++17 release
TARGET = nimrag-sync
TEMPLATE = app

HEADERS += \
    src/backend.h \
    src/nimrag.h \
    src/theme.h \
    src/translator.h

SOURCES += \
    src/main.cpp \
    src/backend.cpp \
    src/theme.cpp \
    src/translator.cpp

RESOURCES += src/resources.qrc

# The Rust core (core/), built by bin/build with cargo before qmake runs.
NIMRAG_CORE = $$PWD/core/target/release/libnimrag.a
LIBS += $$NIMRAG_CORE -ldl -lpthread -lm
PRE_TARGETDEPS += $$NIMRAG_CORE
