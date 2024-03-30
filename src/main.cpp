#include <Arduino.h>
#include <ws_logger.h>
#include <ESP8266WiFi.h>
#include <WiFiManager.h>
#include <WebSocketsClient.h>
#include <Wire.h>
#include <MFRC522v2.h>
#include <MFRC522DriverSPI.h>
#include <MFRC522DriverPinSimple.h>
#include <MFRC522Debug.h>
#include "version.h"
#include "utils.hpp"

#define NAME_PREFIX "FkmTimerSAt"
#define WIFI_PASSWORD "FkmTimer"

#define RFID_CS 16
#define RFID_SCK 14
#define RFID_MISO 12
#define RFID_MOSI 13

#define LED_PIN 4

WebSocketsClient webSocket;

MFRC522DriverPinSimple ss_pin(RFID_CS);
MFRC522DriverSPI driver{ss_pin};
MFRC522 mfrc522{driver};

void initWifi();

void setup() {
  pinMode(LED_PIN, OUTPUT);
  pinMode(0, INPUT_PULLUP);
  digitalWrite(LED_PIN, LOW);

  Serial.begin(115200);
  Logger.begin(&Serial);
  SPI.pins(RFID_SCK, RFID_MISO, RFID_MOSI, RFID_CS);
  mfrc522.PCD_Init();

  initWifi(); // and init ws
}

unsigned long lastCardReadTime = 0;
void loop() {

  // TODO: remove this
  if(digitalRead(0) == LOW) {
    sendAttendance(&webSocket, 65436534, LED_PIN); // FOR TESTING

    while(digitalRead(0) == LOW) {}
  }

  delay(15);

  webSocket.loop();
  Logger.loop();

  if (millis() - lastCardReadTime < 500) return;
  if (!mfrc522.PICC_IsNewCardPresent()) return;
  if (!mfrc522.PICC_ReadCardSerial()) return;

  unsigned long cardId = mfrc522.uid.uidByte[0] + (mfrc522.uid.uidByte[1] << 8) + (mfrc522.uid.uidByte[2] << 16) + (mfrc522.uid.uidByte[3] << 24);
  Logger.printf("Scanned card ID: %lu\n", cardId);
  sendAttendance(&webSocket, cardId, LED_PIN);

  mfrc522.PICC_HaltA();
  lastCardReadTime = millis();
}

void webSocketEvent(WStype_t type, uint8_t *payload, size_t length);
String wsURL = "";

// OTA
int sketchSize = 0;
int sketchSizeRemaining = 0;
int chunksTransfered = 0;
bool update = false;

void initWs() {
  while (true) {
    wsURL = getWsUrl();
    if (wsURL.length() > 0)
      break;
    delay(1000);
  }

  ws_info_t wsInfo = parseWsUrl(wsURL.c_str());

  char finalPath[256];
  snprintf(finalPath, 256, "%s?id=%lu&ver=%s&chip=%s&bt=%s&firmware=%s", 
            wsInfo.path, getEspId(), FIRMWARE_VERSION, CHIP, BUILD_TIME, FIRMWARE_TYPE);

  webSocket.begin(wsInfo.host, wsInfo.port, finalPath);
  webSocket.onEvent(webSocketEvent);
  webSocket.setReconnectInterval(1500);
  Logger.setWsClient(&webSocket);
}

void initWifi() {
  WiFiManager wm;

  char generatedDeviceName[100];
  snprintf(generatedDeviceName, 100, "%s-%x", NAME_PREFIX, (unsigned int)getEspId());

  wm.setConfigPortalTimeout(300);
  // wm.setConfigPortalBlocking(false);

  bool res = wm.autoConnect(generatedDeviceName, WIFI_PASSWORD);
  if (!res) {
    Logger.println("Failed to connect to wifi... Restarting!");
    delay(1500);
    ESP.restart();
  }

  configTime(3600, 0, "pool.ntp.org", "time.nist.gov", "time.google.com");
  initWs();
}

void webSocketEvent(WStype_t type, uint8_t *payload, size_t length) {
  if (type == WStype_TEXT) {
    JsonDocument doc;
    deserializeJson(doc, payload);

    if (doc.containsKey("start_update")) {
      digitalWrite(LED_PIN, LOW);
      if (update) {
        ESP.restart();
      }

      if (doc["start_update"]["esp_id"] != getEspId() ||
          doc["start_update"]["version"] == FIRMWARE_VERSION) {
        Logger.println("Cannot start update! (wrong esp id or same firmware version)");
        return;
      }

      sketchSize = sketchSizeRemaining = doc["start_update"]["size"];
      unsigned long maxSketchSize =
          (ESP.getFreeSketchSpace() - 0x1000) & 0xFFFFF000;

      Logger.printf("[Update] Max Sketch Size: %lu | Sketch size: %d\n",
                    maxSketchSize, sketchSizeRemaining);
      if (!Update.begin(maxSketchSize)) {
        Update.printError(Serial);
        ESP.restart();
      }

      update = true;
      webSocket.sendBIN((uint8_t *)NULL, 0);
    } else if(doc.containsKey("attendance_marked")) {
      if (doc["attendance_marked"]["esp_id"] != getEspId()) {
        Logger.printf("Wrong attendance marked frame!\n");
        return;
      }

      digitalWrite(LED_PIN, HIGH);
    }
  } else if (type == WStype_BIN) {
    if (Update.write(payload, length) != length) {
      Update.printError(Serial);
      Logger.printf("[Update] (lensum) Error! Rebooting...\n");

      delay(250);
      ESP.restart();
    }

    sketchSizeRemaining -= length;
    chunksTransfered++;

    if (chunksTransfered % 10 == 0) {
      digitalWrite(LED_PIN, HIGH);
      delay(50);
      digitalWrite(LED_PIN, LOW);
    }

    if (sketchSizeRemaining <= 0) {
      Logger.printf("[Update] Left 0, delay 1s\n");
      delay(1000);

      if (Update.end(true)) {
        Logger.printf("[Update] Success!!! Rebooting...\n");

        delay(250);
        ESP.restart();
      } else {
        Update.printError(Serial);
        Logger.printf("[Update] Error! Rebooting...\n");

        delay(250);
        ESP.restart();
      }
    }

    webSocket.sendBIN((uint8_t *)NULL, 0);
  } else if (type == WStype_CONNECTED) {
    Serial.println("Connected to WebSocket server");
    sendAddDevice(&webSocket);
    digitalWrite(LED_PIN, HIGH);
  } else if (type == WStype_DISCONNECTED) {
    Serial.println("Disconnected from WebSocket server");
    digitalWrite(LED_PIN, LOW);
  }
}