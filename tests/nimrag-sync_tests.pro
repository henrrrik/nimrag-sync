QT += core gui quick quickcontrols2 testlib dbus concurrent
CONFIG += c++17 testcase
TARGET = nimrag-sync_tests
TEMPLATE = app

INCLUDEPATH += ../src

# The sources, so the catalogs can be checked against every qsTr() and tr().
DEFINES += SOURCE_DIR=\\\"$$PWD/../src\\\"

HEADERS += ../src/backend.h ../src/theme.h ../src/translator.h
SOURCES += nimrag-sync_tests.cpp ../src/backend.cpp ../src/theme.cpp ../src/translator.cpp

# The backend links the Rust core (bin/test builds it first).
LIBS += $$PWD/../core/target/release/libnimrag.a -ldl -lpthread -lm
