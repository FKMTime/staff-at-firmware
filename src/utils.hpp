#ifndef __UTILS_HPP__
#define __UTILS_HPP__

#include <Arduino.h>
#include <ESP8266mDNS.h>
#include <ws_logger.h>
#include <WebSocketsClient.h>

unsigned long getEspId() {
    uint64_t efuse = ESP.getChipId();
    efuse = (~efuse) + (efuse << 18);
    efuse = efuse ^ (efuse >> 31);
    efuse = efuse * 21;
    efuse = efuse ^ (efuse >> 11);
    efuse = efuse + (efuse << 6);
    efuse = efuse ^ (efuse >> 22);

    return (unsigned long)(efuse & 0x000000007FFFFFFF);
}

struct WsInfo {
    char host[100];
    int port;
    char path[100];
} typedef ws_info_t;

ws_info_t parseWsUrl(const char *url) {
  ws_info_t wsInfo = {0};
  int pathPtr = 0;

  if (strncmp("ws://", url, 5) == 0) {
    pathPtr = 5;
    wsInfo.port = 80;
  } else if (strncmp("wss://", url, 6) == 0) {
    pathPtr = 6;
    wsInfo.port = 443;
  } else {
    return wsInfo;
  }

  // url with offset of pathPtr
  char *pathSplitPtr = strchr(url + pathPtr, '/');
  int pathSplitIdx = pathSplitPtr == NULL ? strlen(url) : pathSplitPtr - url;

  if (pathSplitPtr != NULL) {
    strncpy(wsInfo.path, pathSplitPtr, 100);
  }
  strncpy(wsInfo.host, url + pathPtr, pathSplitIdx - pathPtr);

  // snprintf(wsInfo.host, pathSplitIdx - pathPtr + 1, url + pathPtr);
  if (strlen(wsInfo.path) == 0) {
    wsInfo.path[0] = '/';
    wsInfo.path[1] = '\0';
  }

  char *portSplitPtr = strchr(wsInfo.host, ':');
  if (portSplitPtr != NULL) {
    char portStr[10];

    int idx = portSplitPtr - wsInfo.host;
    strcpy(portStr, wsInfo.host + idx + 1);

    wsInfo.host[idx] = '\0';
    wsInfo.port = atoi(portStr);
  }

  return wsInfo;
}

String getWsUrl() {
  if (!MDNS.begin("random")) {
    Logger.printf("Failed to setup MDNS!");
  }

  int n = MDNS.queryService("stackmat", "tcp");
  if (n > 0) {
    Logger.printf("Found stackmat MDNS:\nHostname: %s, IP: %s, PORT: %d\n",
                  MDNS.hostname(0).c_str(), MDNS.IP(0).toString().c_str(),
                  MDNS.port(0));
    return MDNS.hostname(0);
  }
  MDNS.end();

  return "";
}

void sendBatteryStats(WebSocketsClient *webSocket, float level, float voltage) {
  JsonDocument doc;
  doc["battery"]["esp_id"] = getEspId();
  doc["battery"]["level"] = level;
  doc["battery"]["voltage"] = voltage;

  String json;
  serializeJson(doc, json);
  webSocket->sendTXT(json);
}

void sendAddDevice(WebSocketsClient *webSocket) {
  JsonDocument doc;
  doc["add"]["esp_id"] = getEspId();
  doc["add"]["firmware"] = FIRMWARE_TYPE;

  String json;
  serializeJson(doc, json);
  webSocket->sendTXT(json);
}

void sendAttendance(WebSocketsClient *webSocket, unsigned long cardId, int led_pin) {
  JsonDocument doc;
  doc["card_info_request"]["card_id"] = cardId;
  doc["card_info_request"]["esp_id"] = getEspId();
  doc["card_info_request"]["attendance_device"] = true;
  
  String json;
  serializeJson(doc, json);
  webSocket->sendTXT(json);
  
  for(int i = 0; i < 3; i++) {
    digitalWrite(led_pin, LOW);
    delay(100);
    digitalWrite(led_pin, HIGH);
    delay(100);
  }

  digitalWrite(led_pin, LOW);
}

#endif