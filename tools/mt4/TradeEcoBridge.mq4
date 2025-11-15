//+------------------------------------------------------------------+
//|                                               TradeEcoBridge.mq4 |
//|                    Poll HQ for MT4-mapped signals and execute    |
//+------------------------------------------------------------------+
#property strict

input string BaseURL   = "http://127.0.0.1:8088"; // Set HQ_HEALTH_PORT if different
input string SymbolMT4 = "";                      // Leave empty to use current chart symbol
input int    PollSec   = 5;                        // Poll interval (seconds)

bool inited = false;
double lastLots = 0;
string lastSide = "";
datetime lastCheck = 0;

int OnInit() {
  EventSetTimer(PollSec);
  inited = true;
  return(INIT_SUCCEEDED);
}

void OnDeinit(const int reason) {
  EventKillTimer();
}

void OnTimer() {
  if(!inited) return;
  string sym = SymbolMT4;
  if(StringLen(sym)==0) sym = Symbol();
  string url = BaseURL + "/mt4/signals?symbol=" + sym;
  char data[]; string headers; int res = 0; string resp = "";
  res = WebRequest("GET", url, "", NULL, 5000, data, headers);
  if(res == -1) { Print("WebRequest failed: ", GetLastError()); return; }
  resp = CharArrayToString(data, 0, -1);
  if(StringFind(resp, "not_found", 0) >= 0) return;

  // Simple JSON parsing (assumes keys present)
  string side = json_extract(resp, "side");
  string lots_s = json_extract(resp, "lots");
  string sl_s = json_extract(resp, "sl");
  string tp_s = json_extract(resp, "tp");
  double lots = StrToDouble(lots_s);
  double sl = StrToDouble(sl_s);
  double tp = StrToDouble(tp_s);

  // De-duplicate same signal
  if(lots == lastLots && side == lastSide && TimeCurrent()-lastCheck < PollSec) return;

  int type = (side == "BUY") ? OP_BUY : (side == "SELL") ? OP_SELL : -1;
  if(type < 0 || lots <= 0) return;

  double price = (type==OP_BUY) ? Ask : Bid;
  double slv = (sl>0) ? sl : 0;
  double tpv = (tp>0) ? tp : 0;

  int ticket = OrderSend(sym, type, lots, price, 20, slv, tpv, "TradeEco", 0, 0, clrGreen);
  if(ticket < 0) { Print("OrderSend failed: ", GetLastError()); }
  else { lastLots = lots; lastSide = side; lastCheck = TimeCurrent(); }
}

// Minimal JSON value extractor for flat keys
string json_extract(string s, string key) {
  string pat = StringFormat("\"%s\":", key);
  int p = StringFind(s, pat, 0); if(p<0) return "";
  p += StringLen(pat);
  // skip spaces and quotes
  while(p<StringLen(s) && (s[p]==' ' || s[p]=='\"')) p++;
  int q = p;
  while(q<StringLen(s) && s[q] != ',' && s[q] != '}' && s[q] != '"') q++;
  string val = StringSubstr(s, p, q-p);
  return val;
}