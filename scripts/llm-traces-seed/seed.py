#!/usr/bin/env python3
"""Fills a running LLM Traces collector with a week of realistic LLM traffic.

    python3 scripts/llm-traces-seed/seed.py                      # about 1,000 traces
    python3 scripts/llm-traces-seed/seed.py --scale 3            # about 3,000
    python3 scripts/llm-traces-seed/seed.py --url http://localhost:4399

Raise "Keep" on the Connect tab before seeding more than it holds (2,000 by
default), or the oldest seeded traces are trimmed as the rest arrive.

Everything goes over the same HTTP protocols the real SDKs use, so it
exercises the collector end to end:

    Langfuse ingestion   support bot, SQL analytics agent, meeting notes
    LangSmith runs       coding assistant, travel planner (LangGraph-style)
    OpenTelemetry OTLP   docs Q&A (GenAI conventions + OpenInference),
                         inbox summarizer (OpenLLMetry), translations

The conversations are multi-turn: each turn is a trace sharing the session,
and each LLM call carries the whole history so far, as a real chat app sends
it. There are tool calls, retrieval, rate-limit errors and retries, tool
timeouts, user feedback and LLM-judge scores, token counts derived from the
text, costs from list prices, and a few meeting transcripts big enough to hit
the store's 512 KB-per-field cap. Timestamps spread over the last --days days,
weighted to working hours. No model is called; every word is written here.
Standard library only.
"""

import argparse
import base64
import json
import math
import random
import sys
import time
import urllib.error
import urllib.request
import uuid
from dataclasses import dataclass, field
from datetime import datetime, timedelta, timezone
from typing import Any, Optional

# --------------------------------------------------------------------------
# Models: $ per 1M input tokens, $ per 1M output tokens, typical seconds to
# the first token, output tokens per second.

MODELS = {
    "gpt-4o": (2.50, 10.00, 0.45, 70),
    "gpt-4o-mini": (0.15, 0.60, 0.30, 110),
    "gpt-4.1": (2.00, 8.00, 0.50, 75),
    "gpt-4.1-mini": (0.40, 1.60, 0.35, 100),
    "claude-sonnet-4": (3.00, 15.00, 0.80, 55),
    "claude-3-5-haiku": (0.80, 4.00, 0.40, 120),
    "gemini-2.0-flash": (0.10, 0.40, 0.25, 180),
    "gemini-1.5-pro": (1.25, 5.00, 1.40, 60),
    "text-embedding-3-small": (0.02, 0.0, 0.05, 1),
}
PROVIDER = {
    "gpt": "openai",
    "text": "openai",
    "claude": "anthropic",
    "gemini": "google",
}


def provider(model: str) -> str:
    return PROVIDER[model.split("-")[0]]


def tokens(text: str) -> int:
    return max(1, round(len(text) / 3.8))


def text_of(content: Any) -> str:
    if isinstance(content, str):
        return content
    return json.dumps(content)


@dataclass
class Obs:
    """One observation, protocol-neutral. Times are epoch seconds."""

    kind: str
    name: str
    start: float
    end: float
    input: Any = None
    output: Any = None
    model: Optional[str] = None
    params: Optional[dict] = None
    usage: Optional[tuple] = None  # (input tokens, output tokens)
    cost: Optional[tuple] = None  # (input $, output $)
    first_token: Optional[float] = None
    level: Optional[str] = None
    status: Optional[str] = None
    metadata: Optional[dict] = None
    attrs: Optional[dict] = None  # OTel attributes, for OTel scenarios
    children: list = field(default_factory=list)
    id: str = ""


@dataclass
class Trace:
    name: str
    root: Obs
    session: Optional[str] = None
    user: Optional[str] = None
    tags: list = field(default_factory=list)
    metadata: dict = field(default_factory=dict)
    input: Any = None
    output: Any = None
    scores: list = field(default_factory=list)  # (name, value, comment)
    release: Optional[str] = None
    environment: str = "production"


class Gen:
    """Random choices plus ids, from one seed, so a run is reproducible."""

    def __init__(self, seed: int):
        self.r = random.Random(seed)

    def uid(self) -> str:
        return str(uuid.UUID(int=self.r.getrandbits(128), version=4))

    def hex(self, n: int) -> str:
        return "%0*x" % (n, self.r.getrandbits(n * 4))

    def pick(self, items):
        return self.r.choice(items)

    def chance(self, p: float) -> bool:
        return self.r.random() < p

    def between(self, a: float, b: float) -> float:
        return self.r.uniform(a, b)

    def gap(self) -> float:
        """A few milliseconds of code between two steps."""
        return self.r.uniform(0.002, 0.03)


def llm(g: Gen, t: float, name: str, model: str, messages: list, reply: str, *, params=None,
        tool_calls=None, error=None, metadata=None) -> Obs:
    pin, pout, ttft, tps = MODELS[model]
    tin = sum(tokens(text_of(m.get("content", ""))) + 4 for m in messages)
    if error:
        dur = g.between(0.15, 1.2)
        return Obs("generation", name, t, t + dur, input=messages, model=model, params=params,
                   usage=(tin, 0), cost=(0.0, 0.0), level="ERROR", status=error, metadata=metadata)
    out_text = reply + (json.dumps(tool_calls) if tool_calls else "")
    tout = tokens(out_text)
    first = ttft * g.between(0.6, 1.9)
    dur = first + tout / (tps * g.between(0.7, 1.25))
    output = {"role": "assistant", "content": reply}
    if tool_calls:
        output["tool_calls"] = tool_calls
    return Obs("generation", name, t, t + dur, input=messages, output=output, model=model, params=params,
               usage=(tin, tout), cost=(tin * pin / 1e6, tout * pout / 1e6), first_token=t + first,
               metadata=metadata)


def tool(g: Gen, t: float, name: str, args: dict, result: Any, lo=0.04, hi=0.9, error=None) -> Obs:
    dur = g.between(lo, hi)
    if error:
        return Obs("tool", name, t, t + dur, input=args, level="ERROR", status=error)
    return Obs("tool", name, t, t + dur, input=args, output=result)


def wrap(kind: str, name: str, start: float, children: list, input=None, output=None, metadata=None) -> Obs:
    end = max([start] + [c.end for c in children]) + 0.004
    return Obs(kind, name, start, end, input=input, output=output, children=children, metadata=metadata)


def tool_call(g: Gen, name: str, args: dict) -> dict:
    return {"id": "call_" + g.hex(24), "type": "function", "function": {"name": name, "arguments": json.dumps(args)}}


# ==========================================================================
# Scenario 1: Northwind Outfitters support bot (Langfuse)

SUPPORT_SYSTEM = """You are Ava, the customer support assistant for Northwind Outfitters, an online store for hiking and camping gear.

Rules:
- Be warm, brief and specific. Use the customer's first name once.
- Only state order facts that appear in the order data. Never invent tracking numbers, dates or amounts.
- Returns: 60 days from delivery, unworn, free return label. Refunds go back to the original payment method within 5-7 business days of the return being scanned.
- Exchanges: free size exchanges within 60 days; we ship the new size as soon as the return is scanned, or immediately for Summit members.
- Damaged or defective items: apologise, ask for one photo, and offer a replacement or a refund. No need to send damaged items back.
- Warranty: boots and backpacks have a 2-year warranty against defects in materials and workmanship.
- Never promise anything the policies above do not allow. If the customer is upset or asks for a person, offer a handoff to the human team (replies within 4 business hours).
- Do not ask for card numbers or passwords."""

CLASSIFIER_SYSTEM = (
    "Classify the customer's latest message. Answer with JSON only: "
    '{"intent": <one of order_status, return, exchange, damaged_item, promo_code, cancel_order, '
    'warranty, address_change, follow_up, thanks, handoff>, "confidence": <0-1>, '
    '"sentiment": <positive|neutral|negative>}'
)

CUSTOMERS = [
    ("Maya", "Patel", "summit"), ("Jonas", "Becker", "basic"), ("Aisha", "Rahman", "summit"),
    ("Liam", "O'Connor", "basic"), ("Sofia", "Rossi", "basic"), ("Kenji", "Watanabe", "summit"),
    ("Chloe", "Martin", "basic"), ("Diego", "Alvarez", "basic"), ("Fatima", "Haddad", "summit"),
    ("Noah", "Kim", "basic"), ("Ingrid", "Larsen", "basic"), ("Tunde", "Adeyemi", "summit"),
    ("Hannah", "Schultz", "basic"), ("Arjun", "Mehta", "basic"), ("Zoe", "Dubois", "summit"),
    ("Mateo", "Silva", "basic"), ("Priya", "Nair", "basic"), ("Ethan", "Brooks", "basic"),
]
PRODUCTS = [
    ("TrailRunner 3 hiking boots", "boots", 159.00), ("Alpine 45L backpack", "pack", 189.00),
    ("StormShell rain jacket", "jacket", 229.00), ("Basecamp 2-person tent", "tent", 349.00),
    ("MerinoPro base layer", "baselayer", 89.00), ("SummitLite trekking poles", "poles", 119.00),
    ("Ridgeline down vest", "vest", 149.00), ("Cascade waterproof gloves", "gloves", 59.00),
]
DEFECTS = {
    "boots": "the stitching on the left boot is coming apart", "pack": "one of the shoulder straps is torn",
    "jacket": "there's a tear along the zipper", "tent": "a pole section is cracked",
    "baselayer": "there's a hole in the sleeve", "poles": "one of the poles is bent",
    "vest": "the seam is ripped and feathers are coming out", "gloves": "the seam on one thumb is ripped",
}
CITIES = ["Louisville, KY", "Memphis, TN", "Ontario, CA", "Allentown, PA", "Dallas, TX", "Columbus, OH"]
CARRIERS = [("UPS", "1Z"), ("FedEx", "7"), ("USPS", "94")]

KB = {
    "shipping-times": ("Shipping times and tracking",
                       "Standard shipping takes 3-6 business days. Tracking can pause for up to 48 hours between "
                       "hubs; that is normal. If a package shows no movement for 5 business days we open a trace "
                       "with the carrier."),
    "lost-packages": ("Lost or late packages",
                      "If an order hasn't arrived 3 business days after its estimated delivery date, we reship it "
                      "for free or refund it. We can't reship while the carrier still shows the package moving."),
    "returns": ("Returns within 60 days",
                "Unworn items can be returned within 60 days of delivery with a free prepaid label. Refunds are "
                "issued to the original payment method 5-7 business days after the return is scanned."),
    "exchanges": ("Exchanging for a different size",
                  "Size exchanges are free within 60 days. We ship the new size when the return is scanned; Summit "
                  "members get it shipped straight away."),
    "damaged": ("Damaged or defective items",
                "Send one photo of the damage. We'll replace or refund it, whichever you prefer. You don't need to "
                "return a damaged item."),
    "promo-codes": ("Promo codes and gift cards",
                    "Codes apply to full-price items only, one per order, and can't be added after an order is "
                    "placed. Expired codes can't be reactivated."),
    "cancel": ("Cancelling an order",
               "Orders can be cancelled until they are packed, usually within 2 hours of being placed. After that, "
               "the order can be returned for free once it arrives."),
    "warranty": ("Warranty on boots and packs",
                 "Boots and backpacks carry a 2-year warranty against defects in materials and workmanship. Normal "
                 "wear, like worn tread, isn't covered."),
    "address": ("Changing a shipping address",
                "We can change the address until the order is packed. After that, use the carrier's own redirect "
                "service from the tracking page."),
}


# Which products an issue can be about: a sole only peels off boots, and a
# size exchange needs something that comes in sizes.
ISSUE_KINDS = {
    "warranty": {"boots"},
    "exchange": {"boots", "jacket", "baselayer", "vest", "gloves"},
}
ISSUES = ["wismo", "return", "exchange", "damaged", "promo", "cancel", "warranty", "address"]


def support_context(g: Gen, conv_start: float, issue: str) -> dict:
    first, last, tier = g.pick(CUSTOMERS)
    allowed = ISSUE_KINDS.get(issue)
    product, kind, price = g.pick([p for p in PRODUCTS if allowed is None or p[1] in allowed])
    carrier, prefix = g.pick(CARRIERS)
    ordered = datetime.fromtimestamp(conv_start - g.between(3, 12) * 86400, timezone.utc)
    eta = ordered + timedelta(days=g.r.randint(5, 9))
    return {
        "first": first, "last": last, "tier": tier, "product": product, "kind": kind, "price": price,
        "order": "NW-%05d" % g.r.randint(10000, 99999), "carrier": carrier,
        "tracking": prefix + g.hex(16).upper(), "city": g.pick(CITIES),
        "ordered": ordered.strftime("%b %d").replace(" 0", " "), "eta": eta.strftime("%A, %b %d").replace(" 0", " "),
        "size": g.pick(["8", "9", "9.5", "10"]) if kind == "boots" else g.pick(["S", "M", "L"]),
        "user_id": "cust_" + g.hex(10), "email": f"{first.lower()}.{last.lower().replace(chr(39), '')}@example.com",
    }


def order_record(c: dict, status: str = "in_transit", **extra) -> dict:
    rec = {
        "order_id": c["order"], "status": status, "placed": c["ordered"],
        "items": [{"name": c["product"], "qty": 1, "price": c["price"], "size": c["size"]}],
        "total": round(c["price"] * 1.08, 2), "carrier": c["carrier"], "tracking": c["tracking"],
        "last_scan": f"{c['city']} hub", "estimated_delivery": c["eta"], "customer_tier": c["tier"],
    }
    rec.update(extra)
    return rec


def support_script(g: Gen, c: dict, issue: str) -> list:
    """The turns of one support conversation: user text, intent, tool, articles, reply."""
    f, o, p = c["first"], c["order"], c["product"]
    lookup = ("lookup_order", {"order_id": o})
    if issue == "wismo":
        return [
            dict(user=g.pick([
                f"Hi, I ordered the {p} on {c['ordered']} (order {o}) and it still hasn't arrived. Tracking hasn't moved in days.",
                f"where is my order?? {o}. it said it would be here by now",
                f"Hello! Could you check on order {o}? The {c['carrier']} tracking page has been stuck for a while.",
            ]), intent="order_status", tool=(*lookup, order_record(c)), kb=["shipping-times"],
                reply=f"Thanks for your patience, {f}! I checked order {o}: it's with {c['carrier']} (tracking "
                      f"{c['tracking']}), and its last scan was at the {c['city']} hub. The current estimate is "
                      f"{c['eta']}. Tracking often pauses for a day or two between hubs, so this looks normal so far."),
            dict(user=g.pick([
                "Can you just send another one? I need it for a trip this weekend.",
                "That's what it said last week too. Can you reship it?",
                "ok but what happens if it doesn't show up by then",
            ]), intent="follow_up", tool=None, kb=["lost-packages"],
                reply=f"I understand, especially with a trip coming up. While {c['carrier']} still shows the "
                      f"package moving I can't reship it yet, but if it hasn't arrived 3 business days after "
                      f"{c['eta']}, reply here and I'll send a free replacement or refund it, whichever you prefer. "
                      f"I've also asked {c['carrier']} to open a trace on it today."),
            dict(user=g.pick(["ok, thanks", "Thanks Ava, appreciate it.", "Fine. Thanks for checking."]),
                 intent="thanks", tool=None, kb=None,
                 reply=f"You're welcome, {f}! I hope it turns up well before your trip. You can reply to this chat "
                       f"anytime and it'll come straight back to me."),
        ]
    if issue == "return":
        return [
            dict(user=g.pick([
                f"I'd like to return the {p} from order {o}, it's not what I expected.",
                f"How do I return something? Order {o}, the {p}. Never worn.",
            ]), intent="return", tool=(*lookup, order_record(c, "delivered", delivered=c["ordered"])),
                kb=["returns"],
                reply=f"Of course, {f}. The {p} from order {o} is well within our 60-day window. I've emailed a "
                      f"free return label to {c['email']}. Once the package is scanned, your refund of "
                      f"${c['price']:.2f} goes back to your original payment method within 5-7 business days."),
            dict(user=g.pick(["Do I need the original box?", "Can I drop it at any UPS store?",
                              "Will I get the shipping cost back too?"]),
                 intent="follow_up", tool=None, kb=["returns"],
                 reply=g.pick([
                     "No original box needed: any sturdy box or mailer works. Just put the label on the outside "
                     "and cover any old barcodes.",
                     "Yes, any UPS location or drop box works, and you'll get a receipt with a tracking number.",
                     "The return itself is free. Original shipping isn't refunded unless the item was damaged or "
                     "we made a mistake, but there's nothing to pay for sending it back.",
                 ])),
            dict(user="great, thank you", intent="thanks", tool=None, kb=None,
                 reply=f"Happy to help, {f}! If you'd like a different size or color instead, just let me know."),
        ]
    if issue == "exchange":
        boots = c["kind"] == "boots"
        new_size = g.pick(["10", "10.5", "11"]) if boots else g.pick(["L", "XL"])
        item = "pair" if boots else "one"
        member = c["tier"] == "summit"
        return [
            dict(user=f"The {p} {'run' if boots or p.endswith('s') else 'runs'} small. Can I swap for a size {new_size}? Order {o}.",
                 intent="exchange", tool=(*lookup, order_record(c, "delivered")), kb=["exchanges"],
                 reply=f"Absolutely, {f}. A size {new_size} is in stock, and the exchange is free. "
                       + (f"As a Summit member you get the new {item} shipped today, before we receive the return. "
                          if member else
                          "We'll ship the new size as soon as your return is scanned, usually the same day. ")
                       + f"I've emailed the return label to {c['email']}."),
            dict(user="How long will that take in total?", intent="follow_up", tool=None, kb=["shipping-times"],
                 reply=("You should have the new size in 3-6 business days, since it's already on its way."
                        if member else
                        "About a week end to end: a day or two for the return to be scanned, then 3-6 business days "
                        f"for the new {item}.")),
        ]
    if issue == "damaged":
        return [
            dict(user=g.pick([
                f"My {p} arrived damaged: {DEFECTS[c['kind']]}. Order {o}. Really disappointed.",
                f"The {p} came damaged!! {DEFECTS[c['kind']]}. {o}",
            ]), intent="damaged_item", tool=(*lookup, order_record(c, "delivered")), kb=["damaged"],
                reply=f"I'm really sorry, {f}. That's not the condition it should arrive in. Could you send one "
                      f"photo of the damage? Then I'll set up a replacement or a full refund of ${c['price']:.2f}, "
                      f"whichever you prefer, and you won't need to send the damaged one back."),
            dict(user="[photo attached] here it is. I'd like a replacement please.", intent="follow_up",
                 tool=("create_replacement", {"order_id": o, "reason": "damaged_in_transit"},
                       {"replacement_order": "NW-%05d" % g.r.randint(10000, 99999), "ships": "today"}),
                 kb=None,
                 reply=f"Thank you, that's clear. Your replacement {p} ships today and I've upgraded it to "
                       f"express at no charge; you'll get the tracking number by email. Please keep or recycle "
                       f"the damaged one. Sorry again for the hassle."),
        ]
    if issue == "promo":
        code = g.pick(["TRAIL20", "WELCOME15", "SUMMIT10", "FALLGEAR"])
        return [
            dict(user=f"Your promo code {code} isn't working at checkout. It says invalid.", intent="promo_code",
                 tool=("check_promo", {"code": code},
                       {"code": code, "valid": False, "reason": g.pick(["expired", "sale_items_excluded"]),
                        "expired_on": "Oct 31"}), kb=["promo-codes"],
                 reply=f"Thanks for flagging that, {f}. I checked {code}: it ended on Oct 31, so checkout is "
                       f"rejecting it. I'm sorry for the confusion. Codes can't be reactivated, but our newsletter "
                       f"sends a new one most months."),
            dict(user="That's annoying, the email I got yesterday still advertised it.", intent="follow_up",
                 tool=None, kb=None,
                 reply="That's on us, and I'm sorry. I've passed the email to our marketing team. As a one-off "
                       "I've added a 15% code to your account, NWSORRY15, valid for 30 days on full-price items."),
            dict(user="ok that works, thanks", intent="thanks", tool=None, kb=None,
                 reply=f"Glad that helps, {f}. Happy trails!"),
        ]
    if issue == "cancel":
        packed = g.chance(0.5)
        return [
            dict(user=f"Please cancel order {o}, I ordered the wrong color.", intent="cancel_order",
                 tool=(*lookup, order_record(c, "packed" if packed else "processing")), kb=["cancel"],
                 reply=(f"I'm sorry, {f}: order {o} was already packed this morning, so I can't cancel it. "
                        f"As soon as it arrives you can return it for free and order the color you wanted."
                        if packed else
                        f"Done, {f}: order {o} is cancelled and you won't be charged. Would you like me to "
                        f"place it again in the right color?")),
            dict(user="Yes, the green one please." if not packed else "Can't you stop it at the warehouse?",
                 intent="follow_up", tool=None, kb=None,
                 reply=("I can't place orders from this chat, but here's a direct link to the green one with "
                        "your size preselected: northwind.example/p/" + c["kind"] + "?color=green"
                        if not packed else
                        "Once it's packed it's out of the warehouse team's hands, unfortunately. The return will "
                        "be free, and I'll note on your account that it was ordered in the wrong color.")),
        ]
    if issue == "warranty":
        return [
            dict(user=f"The sole on my {p} is peeling off after 5 months. Is that covered?", intent="warranty",
                 tool=(*lookup, order_record(c, "delivered", placed="May 14")), kb=["warranty"],
                 reply=f"I'm sorry, {f}, a sole separating after 5 months isn't normal. It's covered by our "
                       f"2-year warranty against defects. Please send a photo of the sole, and I'll arrange a "
                       f"replacement pair."),
            dict(user="sent the photo. do i have to mail them back?", intent="follow_up",
                 tool=("create_replacement", {"order_id": o, "reason": "warranty_sole_separation"},
                       {"replacement_order": "NW-%05d" % g.r.randint(10000, 99999), "ships": "tomorrow"}),
                 kb=None,
                 reply="Thanks, I can see the separation clearly. No need to send them back. Your replacement "
                       "pair ships tomorrow and you'll get tracking by email."),
        ]
    # address_change
    return [
        dict(user=f"I need to change the delivery address on {o}, I'm moving this week.", intent="address_change",
             tool=(*lookup, order_record(c, g.pick(["processing", "in_transit"]))), kb=["address"],
             reply=f"Sure, {f}. What's the new address? If the order hasn't been packed yet I can change it right "
                   f"away; if it has, {c['carrier']} can redirect it from the tracking page."),
        dict(user="1420 Pine St, Apt 3, Portland OR 97205", intent="follow_up",
             tool=("update_address", {"order_id": o, "address": "1420 Pine St, Apt 3, Portland, OR 97205"},
                   {"updated": True}), kb=None,
             reply="All set: order " + o + " will now go to 1420 Pine St, Apt 3, Portland, OR 97205. "
                   "Good luck with the move!"),
    ]


def support_conversation(g: Gen, start: float) -> list:
    issue = g.pick(ISSUES)
    c = support_context(g, start, issue)
    session = "chat_" + g.hex(12)
    reply_model = "claude-sonnet-4" if g.chance(0.3) else "gpt-4o"
    history = []
    traces = []
    t = start
    turns = support_script(g, c, issue)
    for i, turn in enumerate(turns):
        user_msg = turn["user"]
        t0 = t
        children = []
        cls_msgs = [{"role": "system", "content": CLASSIFIER_SYSTEM}, {"role": "user", "content": user_msg}]
        cls = llm(g, t, "classify-intent", "gpt-4o-mini", cls_msgs,
                  json.dumps({"intent": turn["intent"], "confidence": round(g.between(0.82, 0.99), 2),
                              "sentiment": "negative" if "!!" in user_msg or "?" * 2 in user_msg else "neutral"}),
                  params={"temperature": 0, "response_format": {"type": "json_object"}})
        children.append(cls)
        t = cls.end + g.gap()
        context = [f"Customer: {c['first']} {c['last']} ({c['tier']} member), {c['email']}"]
        if turn["tool"]:
            name, args, result = turn["tool"]
            timeout = g.chance(0.02)
            call = tool(g, t, name, args, result, 0.08, 0.6,
                        error="TimeoutError: orders-api did not answer within 5s" if timeout else None)
            if timeout:
                call.end = t + 5.0
            children.append(call)
            t = call.end + g.gap()
            context.append("Order data: " + (json.dumps(result) if not timeout else "unavailable (lookup timed out)"))
        if turn["kb"]:
            docs = [{"id": k, "title": KB[k][0], "text": KB[k][1], "score": round(g.between(0.71, 0.94), 3)}
                    for k in turn["kb"]]
            ret = Obs("retriever", "search-help-center", t, t + g.between(0.06, 0.35),
                      input={"query": user_msg, "top_k": 3}, output=docs)
            children.append(ret)
            t = ret.end + g.gap()
            context.append("Help center:\n" + "\n".join(f"- {d['title']}: {d['text']}" for d in docs))
        messages = [{"role": "system", "content": SUPPORT_SYSTEM},
                    {"role": "system", "content": "\n\n".join(context)}] + history + [{"role": "user", "content": user_msg}]
        params = {"temperature": 0.4, "max_tokens": 400}
        if g.chance(0.04):
            failed = llm(g, t, "compose-reply", reply_model, messages, "", params=params,
                         error=("RateLimitError: Error code: 429 - Rate limit reached for gpt-4o in organization "
                                "org-northwind on tokens per min (TPM): Limit 30000, Used 29418, Requested 1964."
                                if reply_model == "gpt-4o" else
                                "anthropic.RateLimitError: Error code: 429 - {'type': 'error', 'error': {'type': "
                                "'rate_limit_error', 'message': 'Number of request tokens has exceeded your "
                                "per-minute rate limit'}}"))
            children.append(failed)
            t = failed.end + g.between(1.0, 4.0)  # backoff
        reply = turn["reply"]
        gen = llm(g, t, "compose-reply", reply_model, messages, reply, params=params,
                  metadata={"prompt_version": "support-v14"})
        children.append(gen)
        t = gen.end + g.gap()
        children.append(Obs("event", "pii-filter", t, t, input={"checked": "reply"},
                            output={"redactions": 0}, metadata={"rules": ["card_number", "password"]}))
        root = wrap("span", "handle-message", t0, children, input={"message": user_msg},
                    output={"reply": reply})
        history += [{"role": "user", "content": user_msg}, {"role": "assistant", "content": reply}]
        scores = []
        if g.chance(0.4):
            scores.append(("helpfulness", round(g.between(0.55, 1.0), 2), "LLM judge (gpt-4o-mini)"))
        if i == len(turns) - 1 and g.chance(0.6):
            good = g.chance(0.8)
            scores.append(("user-feedback", 1.0 if good else 0.0,
                           g.pick(["Quick and clear", "Solved it, thanks", "Nice bot"]) if good else
                           g.pick(["Didn't really answer my question", "Wanted a real person", "Too slow"])))
        traces.append(Trace(
            name="support-chat", root=root, session=session, user=c["user_id"],
            tags=["support", "web-widget", reply_model.split("-")[0]],
            metadata={"channel": "web", "locale": "en-US", "turn": i + 1, "experiment": "reply-model-ab",
                      "variant": reply_model},
            input=user_msg, output=reply, scores=scores, release="support-bot@2.14.0"))
        t = root.end + g.between(15, 180)  # the customer reads and types
    return traces


# ==========================================================================
# Scenario 2: coding assistant (LangSmith)

CODE_SYSTEM = ("You are a senior engineer pairing with the user inside their repository. Explain the cause in one "
               "or two sentences, then give the smallest correct fix as a code block. Mention a test when it "
               "matters. Use the repository context; don't invent files that aren't there.")


def docker_log(g: Gen, size: int) -> str:
    lines = []
    ts = datetime(2026, 11, 2, 14, 3, 0, tzinfo=timezone.utc)
    mem = 410.0
    n = 0
    while sum(len(x) + 1 for x in lines) < size:
        ts += timedelta(milliseconds=g.r.randint(40, 900))
        n += 1
        mem = min(1990.0, mem + g.between(-2, 9))
        stamp = ts.strftime("%Y-%m-%dT%H:%M:%S.") + "%03dZ" % (ts.microsecond // 1000)
        kind = g.r.random()
        if kind < 0.7:
            lines.append(f"{stamp} INFO  worker-{g.r.randint(1, 4)} import_batch batch_id={g.hex(8)} "
                         f"rows={g.r.randint(800, 5000)} took={g.r.randint(90, 900)}ms rss={mem:.0f}MiB")
        elif kind < 0.9:
            lines.append(f"{stamp} DEBUG csv_reader chunk={n} offset={n * 65536} buffered_rows={g.r.randint(5000, 90000)}")
        else:
            lines.append(f"{stamp} WARN  memory usage {mem / 20.48:.0f}% ({mem:.0f}MiB of 2048MiB) - "
                         "consider lowering IMPORT_CHUNK_ROWS")
    lines += [
        f"{ts.strftime('%Y-%m-%dT%H:%M:%S')}.998Z ERROR worker-2 import_batch failed: MemoryError",
        "Killed",
        "",
        "$ docker inspect importer --format '{{.State.ExitCode}} {{.State.OOMKilled}}'",
        "137 true",
    ]
    return "\n".join(lines)


EXPLAIN = """                                                         QUERY PLAN
-------------------------------------------------------------------------------------------------------------------------------
 Limit  (cost=184231.12..184231.37 rows=100 width=72) (actual time=3412.881..3412.902 rows=100 loops=1)
   ->  Sort  (cost=184231.12..184498.51 rows=106956 width=72) (actual time=3412.879..3412.893 rows=100 loops=1)
         Sort Key: o.created_at DESC
         Sort Method: top-N heapsort  Memory: 39kB
         ->  Hash Join  (cost=9821.44..180143.08 rows=106956 width=72) (actual time=211.402..3371.150 rows=104882 loops=1)
               Hash Cond: (o.customer_id = c.id)
               ->  Seq Scan on orders o  (cost=0.00..167915.60 rows=106956 width=48) (actual time=0.041..3061.774 rows=104882 loops=1)
                     Filter: ((status)::text = 'pending'::text)
                     Rows Removed by Filter: 5893118
               ->  Hash  (cost=6321.88..6321.88 rows=279964 width=32) (actual time=208.612..208.613 rows=280000 loops=1)
                     Buckets: 524288  Batches: 1  Memory Usage: 20141kB
                     ->  Seq Scan on customers c  (cost=0.00..6321.88 rows=279964 width=32) (actual time=0.012..92.130 rows=280000 loops=1)
 Planning Time: 0.412 ms
 Execution Time: 3413.077 ms
(14 rows)"""

CODE_CASES = [
    {
        "repo": "acme/orders-api", "lang": "python",
        "files": [("api/orders.py", "@bp.get('/orders/<int:order_id>')\ndef get_order(order_id):\n    order = db.get_order(order_id)\n    return jsonify(order['items'])"),
                  ("db/queries.py", "def get_order(order_id):\n    row = conn.execute(SELECT_ORDER, (order_id,)).fetchone()\n    return dict(row) if row else None")],
        "turns": [
            ("I'm getting this in production when someone opens an old link:\n\n```\nTraceback (most recent call last):\n"
             "  File \"/app/.venv/lib/python3.12/site-packages/flask/app.py\", line 1511, in wsgi_app\n"
             "    response = self.full_dispatch_request()\n"
             "  File \"/app/.venv/lib/python3.12/site-packages/flask/app.py\", line 919, in full_dispatch_request\n"
             "    rv = self.handle_user_exception(e)\n"
             "  File \"/app/.venv/lib/python3.12/site-packages/flask/app.py\", line 917, in full_dispatch_request\n"
             "    rv = self.dispatch_request()\n"
             "  File \"/app/api/orders.py\", line 42, in get_order\n"
             "    return jsonify(order['items'])\n"
             "TypeError: 'NoneType' object is not subscriptable\n```\nWhat's going on?",
             "`db.get_order` returns `None` when no row matches, and `get_order` indexes into it unconditionally, so "
             "any deleted or mistyped order id turns into a 500. Return a 404 instead:\n\n```python\n"
             "@bp.get('/orders/<int:order_id>')\ndef get_order(order_id):\n    order = db.get_order(order_id)\n"
             "    if order is None:\n        abort(404, description=f'order {order_id} not found')\n"
             "    return jsonify(order['items'])\n```\n\nFlask's `abort` turns that into a JSON 404 through your "
             "existing error handler in `api/errors.py`.", None),
            ("Can you write a test for that?",
             "```python\ndef test_missing_order_returns_404(client, monkeypatch):\n"
             "    monkeypatch.setattr(db, 'get_order', lambda order_id: None)\n"
             "    resp = client.get('/orders/999999')\n    assert resp.status_code == 404\n"
             "    assert resp.json['error'] == 'order 999999 not found'\n\n\n"
             "def test_existing_order_returns_items(client, monkeypatch):\n"
             "    monkeypatch.setattr(db, 'get_order', lambda order_id: {'items': [{'sku': 'A1', 'qty': 2}]})\n"
             "    resp = client.get('/orders/1')\n    assert resp.status_code == 200\n"
             "    assert resp.json == [{'sku': 'A1', 'qty': 2}]\n```\n\nI ran them against your branch; both pass.",
             ("run_tests", {"path": "tests/test_orders.py", "args": "-q"},
              "..                                                                       [100%]\n"
              "2 passed in 0.41s")),
            ("Nice. The PUT endpoint probably has the same problem, right?",
             "Yes: `update_order` calls `order['status']` on the same lookup. Same guard:\n\n```python\n"
             "    order = db.get_order(order_id)\n    if order is None:\n        abort(404, description=f'order "
             "{order_id} not found')\n```\n\nWorth pulling that into a `get_order_or_404` helper so the next "
             "endpoint gets it for free.", None),
        ],
    },
    {
        "repo": "acme/ledger", "lang": "rust",
        "files": [("src/ledger.rs", "pub fn post(&mut self, entry: Entry) {\n    let account = self.accounts.get_mut(&entry.account).unwrap();\n    for hook in &self.hooks { hook(self, &entry); }\n    account.balance += entry.amount;\n}")],
        "turns": [
            ("rustc won't take this:\n\n```\nerror[E0502]: cannot borrow `*self` as immutable because it is also "
             "borrowed as mutable\n  --> src/ledger.rs:41:39\n   |\n40 |     let account = "
             "self.accounts.get_mut(&entry.account).unwrap();\n   |                   ------------- mutable borrow "
             "occurs here\n41 |     for hook in &self.hooks { hook(self, &entry); }\n   |                         "
             "              ^^^^ immutable borrow occurs here\n42 |     account.balance += entry.amount;\n   |     "
             "------------------------------- mutable borrow later used here\n```",
             "`account` holds a mutable borrow of `self.accounts` until line 42, and the hooks need all of `self` "
             "in between. Run the hooks first, then take the mutable borrow only for the update:\n\n```rust\n"
             "pub fn post(&mut self, entry: Entry) {\n    for hook in &self.hooks {\n        hook(self, &entry);\n"
             "    }\n    let account = self.accounts.get_mut(&entry.account).expect(\"unknown account\");\n"
             "    account.balance += entry.amount;\n}\n```\n\nIf a hook must see the *new* balance, apply the "
             "change first and call the hooks after the borrow ends.", None),
            ("hooks do need the new balance actually",
             "Then scope the mutable borrow so it ends before the loop:\n\n```rust\npub fn post(&mut self, entry: "
             "Entry) {\n    {\n        let account = self.accounts.get_mut(&entry.account).expect(\"unknown "
             "account\");\n        account.balance += entry.amount;\n    }\n    for hook in &self.hooks {\n"
             "        hook(self, &entry);\n    }\n}\n```",
             ("cargo_check", {"package": "ledger"}, "    Checking ledger v0.4.2 (/repo)\n    Finished `dev` "
              "profile [unoptimized + debuginfo] target(s) in 1.84s")),
        ],
    },
    {
        "repo": "acme/dashboard", "lang": "typescript",
        "files": [("src/hooks/useFilteredOrders.ts", "export function useFilteredOrders(filters: Filters) {\n  const [rows, setRows] = useState<Order[]>([]);\n  const query = { ...filters, limit: 50 };\n  useEffect(() => {\n    fetchOrders(query).then(setRows);\n  }, [query]);\n  return rows;\n}")],
        "turns": [
            ("This hook hammers our API, hundreds of requests a second in the network tab. Why?",
             "`query` is a new object on every render, so `[query]` always looks changed: the effect fetches, "
             "`setRows` re-renders, a new `query` is built, and it loops. Depend on the values instead:\n\n```ts\n"
             "export function useFilteredOrders(filters: Filters) {\n  const [rows, setRows] = useState<Order[]>([]);\n"
             "  const key = JSON.stringify(filters);\n  useEffect(() => {\n    let cancelled = false;\n"
             "    fetchOrders({ ...filters, limit: 50 }).then((r) => {\n      if (!cancelled) setRows(r);\n    });\n"
             "    return () => {\n      cancelled = true;\n    };\n    // eslint-disable-next-line "
             "react-hooks/exhaustive-deps\n  }, [key]);\n  return rows;\n}\n```\n\nThe `cancelled` flag also "
             "stops a slow, stale response from overwriting a newer one.", None),
            ("Wouldn't useMemo be cleaner than stringify?",
             "It would if the caller memoizes `filters`; then `useMemo(() => ({ ...filters, limit: 50 }), "
             "[filters])` plus `[query]` works. Right now `OrdersPage` builds `filters` inline, so useMemo here "
             "alone wouldn't stop the loop. Either memoize at the call site or keep the key.", None),
        ],
    },
    {
        "repo": "acme/orders-api", "lang": "sql",
        "files": [("migrations/0042_orders.sql", "CREATE TABLE orders (\n  id bigserial PRIMARY KEY,\n  customer_id bigint NOT NULL REFERENCES customers(id),\n  status text NOT NULL,\n  created_at timestamptz NOT NULL DEFAULT now()\n);")],
        "turns": [
            ("The pending orders page takes 3+ seconds. Here's the plan:\n\n```\n" + EXPLAIN + "\n```",
             "Postgres reads all 6M orders to find the 105k pending ones (`Rows Removed by Filter: 5893118`), then "
             "sorts. A partial index that matches both the filter and the sort fixes it:\n\n```sql\n"
             "CREATE INDEX CONCURRENTLY orders_pending_created_idx\n    ON orders (created_at DESC)\n"
             "    WHERE status = 'pending';\n```\n\nWith `LIMIT 100`, the planner can then walk the index and stop "
             "after 100 rows instead of sorting 105k.", None),
            ("Added it. Now it's:\n\n```\n Limit  (cost=0.43..58.71 rows=100 width=72) (actual time=0.052..0.611 "
             "rows=100 loops=1)\n   ->  Nested Loop  (actual time=0.051..0.598 rows=100 loops=1)\n         ->  "
             "Index Scan using orders_pending_created_idx on orders o  (actual time=0.031..0.142 rows=100 loops=1)\n"
             " Execution Time: 0.648 ms\n```\nAnything else?",
             "That's the plan you want: 3.4 s down to 0.65 ms. Two follow-ups: run `ANALYZE orders;` after the "
             "backfill so estimates stay right, and if `status` has only a handful of values, a CHECK constraint "
             "or enum keeps typos like 'Pending' out of the partial index's reach.", None),
        ],
    },
    {
        "repo": "acme/importer", "lang": "python",
        "files": [("importer/run.py", "def import_file(path):\n    df = pd.read_csv(path)\n    for chunk in np.array_split(df, 20):\n        upsert(chunk)")],
        "turns": [
            None,  # filled with a big pasted log
            ("Can't we just raise the container memory?",
             "You can, as a stopgap (`mem_limit: 4g` in compose), but the file sizes keep growing, so you'd be "
             "back here next quarter. Streaming keeps memory flat regardless of file size:\n\n```python\n"
             "def import_file(path, chunk_rows=20_000):\n    for chunk in pd.read_csv(path, chunksize=chunk_rows):\n"
             "        upsert(chunk)\n```\n\n`read_csv(chunksize=...)` never holds more than one chunk.", None),
        ],
    },
]


def code_conversation(g: Gen, start: float) -> list:
    case = g.pick(CODE_CASES)
    thread = g.uid()
    user = "dev_" + g.pick(["mpatel", "jbecker", "arahman", "lnguyen", "sdubois", "tkowalski"])
    model = "claude-sonnet-4" if g.chance(0.75) else "gpt-4.1"
    history = []
    traces = []
    t = start
    for i, turn in enumerate(case["turns"]):
        if turn is None:
            log = docker_log(g, g.r.randint(30_000, 140_000))
            turn = ("Our importer container keeps dying overnight. Full log:\n\n```\n" + log + "\n```",
                    "Exit code 137 with `OOMKilled: true` means the kernel killed it at the 2 GiB limit. The log "
                    "shows RSS climbing steadily across batches: `pd.read_csv(path)` loads the whole file before "
                    "`array_split` divides it, so memory is proportional to the file, not the batch.", None)
        user_msg, reply, tool_spec = turn
        t0 = t
        children = []
        ctx_start = t
        docs = [{"page_content": code, "metadata": {"path": path, "repo": case["repo"]}} for path, code in case["files"]]
        ret = Obs("retriever", "search_codebase", t, t + g.between(0.08, 0.5),
                  input={"query": user_msg[:300], "k": 4}, output={"documents": docs})
        t = ret.end + g.gap()
        build = wrap("chain", "build_context", ctx_start, [ret], input={"question": user_msg[:300]},
                     output={"files": [p for p, _ in case["files"]]})
        children.append(build)
        context = "\n\n".join(f"# {p}\n```\n{c}\n```" for p, c in case["files"])
        messages = ([{"role": "system", "content": CODE_SYSTEM + "\n\nRepository context:\n" + context}]
                    + history + [{"role": "user", "content": user_msg}])
        if g.chance(0.05):
            fail = llm(g, t, "ChatAnthropic" if model.startswith("claude") else "ChatOpenAI", model, messages, "",
                       error="anthropic.APIStatusError: Error code: 529 - {'type': 'error', 'error': {'type': "
                             "'overloaded_error', 'message': 'Overloaded'}}" if model.startswith("claude") else
                       "openai.APITimeoutError: Request timed out.")
            children.append(fail)
            t = fail.end + g.between(2, 6)
        gen = llm(g, t, "ChatAnthropic" if model.startswith("claude") else "ChatOpenAI", model, messages, reply,
                  params={"temperature": 0.2, "max_tokens": 4096})
        children.append(gen)
        t = gen.end + g.gap()
        if tool_spec:
            name, args, out = tool_spec
            run = tool(g, t, name, args, out, 0.9, 6.0)
            children.append(run)
            t = run.end + g.gap()
        root = wrap("chain", "CodeAssistant", t0, children, input={"input": user_msg}, output={"output": reply})
        history += [{"role": "user", "content": user_msg}, {"role": "assistant", "content": reply}]
        scores = []
        if g.chance(0.35):
            scores.append(("user_score", 1.0 if g.chance(0.85) else 0.0, None))
        traces.append(Trace(name="CodeAssistant", root=root, session=thread, user=user,
                             tags=["ide", case["lang"]],
                             metadata={"thread_id": thread, "user_id": user, "repo": case["repo"],
                                       "editor": g.pick(["vscode", "jetbrains", "neovim"])},
                             input={"input": user_msg}, output={"output": reply}, scores=scores))
        t = root.end + g.between(40, 400)
    return traces


# ==========================================================================
# Scenario 3: docs Q&A over "Harbor", a deploy CLI (OTel, GenAI + OpenInference)

HARBOR_DOCS = {
    "install": "Install Harbor with `curl -fsSL https://get.harbor.example | sh` or `brew install harbor`. The CLI "
               "needs Docker 24+ or Podman 4.6+ on the machine that builds images.",
    "config": "harbor.toml lives at the repository root. [app] sets name and region; [build] sets dockerfile and "
              "context; [deploy] sets strategy (rolling or blue-green), min_instances and max_instances.",
    "secrets": "Secrets are set with `harbor secrets set KEY=value` and are injected as environment variables at "
               "runtime. They are encrypted at rest and never printed by `harbor logs`.",
    "rollback": "`harbor rollback` returns to the previous release in under 30 seconds. `harbor releases` lists "
                "the last 20 releases; `harbor rollback --to v42` picks one.",
    "healthchecks": "Deploys wait for the health check at [deploy].health_path (default /healthz) to return 2xx "
                    "for 3 consecutive probes, 5 seconds apart, before shifting traffic.",
    "domains": "`harbor domains add app.example.com` provisions a TLS certificate automatically once the CNAME "
               "points at <app>.harbor.app. Wildcard domains need a DNS-01 challenge.",
    "scaling": "Autoscaling uses CPU by default: scale out above 70% for 2 minutes, scale in below 30% for 10 "
               "minutes. Set [deploy].scale_metric = \"rps\" to scale on requests per second instead.",
    "logs": "`harbor logs -f` streams logs from every instance; `--since 1h` and `--grep ERROR` filter them. Logs "
            "are kept for 7 days on the free plan and 30 days on Team.",
    "preview": "Every pull request gets a preview environment at pr-<number>.<app>.harbor.app when "
               "[preview].enabled = true. Previews sleep after 30 minutes without traffic.",
    "db": "`harbor db create --engine postgres --size small` creates a managed database and sets DATABASE_URL as "
          "a secret. Backups run nightly and are kept for 7 days.",
}
DOCS_QA = [
    ("How do I roll back a bad deploy?", ["rollback"],
     "Run `harbor rollback`: it returns to the previous release in under 30 seconds. To pick an older one, list "
     "them with `harbor releases` and run `harbor rollback --to v42`.",
     ("Does that also roll back my secrets?", ["secrets", "rollback"],
      "No. Secrets aren't versioned with releases: a rollback changes the image and harbor.toml settings, but "
      "the current secrets stay in place. If a release depended on a new secret value, set it back with "
      "`harbor secrets set`.")),
    ("Why is my deploy stuck at 'waiting for health check'?", ["healthchecks"],
     "Harbor waits for /healthz (or [deploy].health_path) to return 2xx three times in a row, 5 seconds apart. "
     "If your app serves health on another path, set health_path in harbor.toml; if it takes long to boot, "
     "check `harbor logs -f` for startup errors.",
     ("My health endpoint is /api/health", ["config", "healthchecks"],
      "Add it under [deploy] in harbor.toml:\n\n```toml\n[deploy]\nhealth_path = \"/api/health\"\n```\n\nThen "
      "redeploy; the next rollout will probe that path.")),
    ("How do I add a custom domain with HTTPS?", ["domains"],
     "Run `harbor domains add app.example.com`, then point a CNAME for app.example.com at <app>.harbor.app. "
     "Harbor provisions the TLS certificate automatically once the CNAME resolves.", None),
    ("Can I scale on requests per second instead of CPU?", ["scaling", "config"],
     "Yes. Set `scale_metric = \"rps\"` under [deploy] in harbor.toml. By default Harbor scales on CPU (out "
     "above 70% for 2 minutes, in below 30% for 10).",
     ("what's the max number of instances?", ["config", "scaling"],
      "It's whatever you set as [deploy].max_instances in harbor.toml; the docs don't state a platform-wide "
      "ceiling, so check your plan's limits in the dashboard if you need more than a few dozen.")),
    ("How long are logs kept?", ["logs"],
     "7 days on the free plan and 30 days on Team. `harbor logs --since 1h --grep ERROR` filters what you "
     "stream.", None),
    ("How do I set an API key for my app without committing it?", ["secrets"],
     "Use `harbor secrets set API_KEY=...`. Secrets are encrypted at rest, injected as environment variables "
     "at runtime, and never printed by `harbor logs`.", None),
    ("Do pull requests get their own environment?", ["preview"],
     "Yes, with [preview].enabled = true each PR gets pr-<number>.<app>.harbor.app. Previews sleep after 30 "
     "minutes without traffic and wake on the next request.",
     ("can previews use their own database?", ["preview", "db"],
      "The docs I have don't say previews get separate databases automatically. You can create one with "
      "`harbor db create`, but how to attach it per preview isn't covered here, so I'd check with support "
      "before relying on it.")),
    ("How do I create a Postgres database?", ["db"],
     "`harbor db create --engine postgres --size small` creates a managed database and stores its connection "
     "string as the DATABASE_URL secret. Backups run nightly and are kept for 7 days.", None),
    ("What do I need installed to use harbor?", ["install"],
     "The Harbor CLI (`brew install harbor` or the install script) plus Docker 24+ or Podman 4.6+ on the "
     "machine that builds images.", None),
    ("Can harbor do blue-green deploys?", ["config"],
     "Yes: set `strategy = \"blue-green\"` under [deploy] in harbor.toml. The default is a rolling deploy.",
     None),
]
DOCS_SYSTEM = ("Answer questions about the Harbor CLI using only the documentation excerpts provided. Cite the "
               "command or setting. If the excerpts don't cover it, say so instead of guessing.")


def genai_messages(messages: list) -> str:
    """The GenAI conventions' message format: role plus typed parts."""
    return json.dumps([{"role": m["role"], "parts": [{"type": "text", "content": m["content"]}]} for m in messages])


def docs_conversation(g: Gen, start: float) -> list:
    q, keys, answer, follow = g.pick(DOCS_QA)
    turns = [(q, keys, answer)] + ([follow] if follow and g.chance(0.7) else [])
    session = "docs_" + g.hex(10)
    user = "anon_" + g.hex(8)
    model = "gpt-4o-mini"
    history = []
    traces = []
    t = start
    for i, (question, keys, ans) in enumerate(turns):
        t0 = t
        emb_in = tokens(question)
        emb = Obs("embedding", "embeddings text-embedding-3-small", t, t + g.between(0.05, 0.2), attrs={
            "gen_ai.operation.name": "embeddings", "gen_ai.system": "openai",
            "gen_ai.request.model": "text-embedding-3-small", "gen_ai.usage.input_tokens": emb_in})
        t = emb.end + g.gap()
        hits = keys + [k for k in g.r.sample(list(HARBOR_DOCS), 3) if k not in keys]
        hits = hits[:4]
        docs = [{"id": f"docs/{k}.md", "score": round(g.between(0.62, 0.91) - j * 0.05, 3), "text": HARBOR_DOCS[k]}
                for j, k in enumerate(hits)]
        ret = Obs("retriever", "retrieve harbor-docs", t, t + g.between(0.04, 0.25), attrs={
            "openinference.span.kind": "RETRIEVER", "input.value": question,
            "output.value": json.dumps(docs), "retrieval.top_k": 4, "db.system": "qdrant"})
        t = ret.end + g.gap()
        excerpts = "\n\n".join(f"[{d['id']}]\n{d['text']}" for d in docs)
        messages = ([{"role": "system", "content": DOCS_SYSTEM + "\n\nDocumentation:\n" + excerpts}] + history
                    + [{"role": "user", "content": question}])
        gen = llm(g, t, "chat " + model, model, messages, ans)
        failed = g.chance(0.02)
        attrs = {
            "gen_ai.operation.name": "chat", "gen_ai.system": "openai", "gen_ai.request.model": model,
            "gen_ai.response.model": model + "-2024-07-18", "gen_ai.request.temperature": 0.1,
            "gen_ai.request.max_tokens": 600, "gen_ai.usage.input_tokens": gen.usage[0],
            "gen_ai.input.messages": genai_messages(messages),
        }
        if failed:
            gen.end = t + g.between(10, 30)
            gen.level, gen.status = "ERROR", "openai.APITimeoutError: Request timed out."
        else:
            attrs.update({"gen_ai.usage.output_tokens": gen.usage[1],
                          "gen_ai.output.messages": genai_messages([{"role": "assistant", "content": ans}]),
                          "gen_ai.response.finish_reasons": ["stop"]})
        gen.attrs = attrs
        t = gen.end + g.gap()
        root = wrap("span", "POST /v1/chat", t0, [emb, ret, gen])
        root.attrs = {"http.request.method": "POST", "http.route": "/v1/chat",
                      "http.response.status_code": 500 if failed else 200, "session.id": session, "user.id": user,
                      "input.value": question, **({} if failed else {"output.value": ans})}
        root.level = "ERROR" if failed else None
        root.status = "upstream model timed out" if failed else None
        history += [{"role": "user", "content": question}, {"role": "assistant", "content": ans}]
        traces.append(Trace(name="POST /v1/chat", root=root))
        t = root.end + g.between(20, 200)
    return traces


# ==========================================================================
# Scenario 4: SQL analytics agent (Langfuse, agent + tool calls)

ANALYTICS_SYSTEM = """You are a data analyst agent for Northwind's warehouse (PostgreSQL 16). Tables:
- orders(id, customer_id, region, channel, created_at, total_usd, status)
- order_items(order_id, product_id, qty, unit_price_usd)
- products(id, name, category)
- customers(id, segment, signup_date, country)
Call run_sql to query. Use only SELECT. Prefer date_trunc for periods. When you have the numbers, answer in 2-4 sentences with a small markdown table."""

ANALYTICS = [
    ("What was revenue by region last quarter?",
     "SELECT region, round(sum(total_usd)) AS revenue\nFROM orders\nWHERE status = 'completed'\n  AND created_at >= date_trunc('quarter', now()) - interval '3 months'\n  AND created_at < date_trunc('quarter', now())\nGROUP BY region\nORDER BY revenue DESC;",
     [{"region": "NA", "revenue": 2841230}, {"region": "EMEA", "revenue": 1592410}, {"region": "APAC", "revenue": 903118}, {"region": "LATAM", "revenue": 311904}],
     "Q3 revenue was $5.65M. North America led with $2.84M (50%), then EMEA at $1.59M and APAC at $0.90M; LATAM was smallest at $0.31M.\n\n| Region | Revenue |\n|---|---|\n| NA | $2,841,230 |\n| EMEA | $1,592,410 |\n| APAC | $903,118 |\n| LATAM | $311,904 |",
     ("Break EMEA down by month", "SELECT date_trunc('month', created_at)::date AS month, round(sum(total_usd)) AS revenue\nFROM orders\nWHERE status = 'completed' AND region = 'EMEA'\n  AND created_at >= date_trunc('quarter', now()) - interval '3 months'\n  AND created_at < date_trunc('quarter', now())\nGROUP BY 1 ORDER BY 1;",
      [{"month": "2026-07-01", "revenue": 611204}, {"month": "2026-08-01", "revenue": 448930}, {"month": "2026-09-01", "revenue": 532276}],
      "EMEA dipped in August ($449k, down 27% from July's $611k) and partly recovered in September ($532k). The August drop lines up with the usual summer slowdown in Southern Europe.\n\n| Month | Revenue |\n|---|---|\n| Jul | $611,204 |\n| Aug | $448,930 |\n| Sep | $532,276 |")),
    ("Which 5 products had the most returns this year?",
     "SELECT p.name, count(*) AS returns\nFROM orders o\nJOIN order_items oi ON oi.order_id = o.id\nJOIN products p ON p.id = oi.product_id\nWHERE o.status = 'returned' AND o.created_at >= date_trunc('year', now())\nGROUP BY p.name ORDER BY returns DESC LIMIT 5;",
     [{"name": "TrailRunner 3 hiking boots", "returns": 1842}, {"name": "StormShell rain jacket", "returns": 1210}, {"name": "MerinoPro base layer", "returns": 966}, {"name": "Ridgeline down vest", "returns": 712}, {"name": "Cascade waterproof gloves", "returns": 655}],
     "TrailRunner 3 boots lead returns by a wide margin (1,842), followed by the StormShell jacket (1,210). Footwear and apparel make up all five, which usually points at sizing rather than quality.\n\n| Product | Returns |\n|---|---|\n| TrailRunner 3 hiking boots | 1,842 |\n| StormShell rain jacket | 1,210 |\n| MerinoPro base layer | 966 |\n| Ridgeline down vest | 712 |\n| Cascade waterproof gloves | 655 |",
     ("What's the return rate for the boots?", "SELECT round(100.0 * count(*) FILTER (WHERE o.status = 'returned') / count(*), 1) AS return_rate_pct\nFROM orders o JOIN order_items oi ON oi.order_id = o.id JOIN products p ON p.id = oi.product_id\nWHERE p.name = 'TrailRunner 3 hiking boots' AND o.created_at >= date_trunc('year', now());",
      [{"return_rate_pct": 14.2}],
      "The TrailRunner 3 return rate this year is 14.2%, well above the 6-8% typical for footwear. Worth checking the size guide.")),
    ("How many new customers signed up per week in October?",
     "SELECT date_trunc('week', signup_date)::date AS week, count(*) AS signups\nFROM customers\nWHERE signup_date >= '2026-10-01' AND signup_date < '2026-11-01'\nGROUP BY 1 ORDER BY 1;",
     [{"week": "2026-09-28", "signups": 1204}, {"week": "2026-10-05", "signups": 1388}, {"week": "2026-10-12", "signups": 2961}, {"week": "2026-10-19", "signups": 1517}, {"week": "2026-10-26", "signups": 1460}],
     "October signups ran around 1,200-1,500 a week, with a spike to 2,961 in the week of Oct 12, about double the norm. That week matches the fall sale launch.\n\n| Week of | Signups |\n|---|---|\n| Sep 28 | 1,204 |\n| Oct 5 | 1,388 |\n| Oct 12 | 2,961 |\n| Oct 19 | 1,517 |\n| Oct 26 | 1,460 |",
     None),
    ("What's our average order value by channel?",
     "SELECT channel, round(avg(total_usd), 2) AS aov, count(*) AS orders\nFROM orders WHERE status = 'completed' AND created_at >= now() - interval '90 days'\nGROUP BY channel ORDER BY aov DESC;",
     [{"channel": "retail", "aov": 162.40, "orders": 21840}, {"channel": "web", "aov": 118.75, "orders": 96312}, {"channel": "app", "aov": 104.10, "orders": 58220}],
     "Over the last 90 days, retail orders have the highest AOV ($162.40), web is at $118.75 and the app lowest at $104.10, though web has by far the most orders (96k).\n\n| Channel | AOV | Orders |\n|---|---|---|\n| retail | $162.40 | 21,840 |\n| web | $118.75 | 96,312 |\n| app | $104.10 | 58,220 |",
     ("Why is app AOV lower?", None, None,
      "The data I can query shows the gap but not the cause. Two likely drivers to check: the app's audience skews to repeat buyers of consumables (socks, gloves), and app-only promo codes discount full-price items. I'd compare category mix by channel next.")),
]


def analytics_conversation(g: Gen, start: float) -> list:
    question, sql, rows, answer, follow = g.pick(ANALYTICS)
    turns = [(question, sql, rows, answer)] + ([follow] if follow and g.chance(0.75) else [])
    session = "analytics_" + g.hex(10)
    user = g.pick(["ana.lopez", "rob.chen", "nina.k", "sam.okafor"]) + "@northwind.example"
    model = "gpt-4.1"
    history = []
    traces = []
    t = start
    for q, sql_text, result, ans in turns:
        t0 = t
        children = []
        messages = [{"role": "system", "content": ANALYTICS_SYSTEM}] + history + [{"role": "user", "content": q}]
        if sql_text:
            bad = g.chance(0.15) and "date_trunc('quarter'" in sql_text
            attempts = [sql_text.replace("date_trunc('quarter'", "date_trunc(quarter") if bad else sql_text]
            if bad:
                attempts.append(sql_text)
            for n, attempt in enumerate(attempts):
                call = tool_call(g, "run_sql", {"query": attempt})
                plan = llm(g, t, "plan-query", model, messages, "", tool_calls=[call],
                           params={"temperature": 0, "tool_choice": "auto"})
                children.append(plan)
                t = plan.end + g.gap()
                failing = bad and n == 0
                run = tool(g, t, "run_sql", {"query": attempt}, {"rows": result, "row_count": len(result)},
                           0.12, 2.8, error='psycopg.errors.UndefinedColumn: column "quarter" does not exist\n'
                                         "LINE 4:   AND created_at >= date_trunc(quarter, now()) - interval...\n"
                           if failing else None)
                children.append(run)
                t = run.end + g.gap()
                messages = messages + [{"role": "assistant", "content": "", "tool_calls": [call]},
                                       {"role": "tool", "tool_call_id": call["id"],
                                        "content": run.status if failing else json.dumps(result)}]
        final = llm(g, t, "answer", model, messages, ans, params={"temperature": 0.2})
        children.append(final)
        t = final.end + g.gap()
        root = wrap("agent", "analytics-agent", t0, children, input=q, output=ans)
        history += [{"role": "user", "content": q}, {"role": "assistant", "content": ans}]
        traces.append(Trace(name="analytics-question", root=root, session=session, user=user,
                            tags=["analytics", "sql-agent"], metadata={"warehouse": "pg-analytics-prod",
                                                                       "max_steps": 6},
                            input=q, output=ans, release="analyst@0.9.3"))
        t = root.end + g.between(30, 300)
    return traces


# ==========================================================================
# Scenario 5: travel planner (LangSmith, LangGraph-style agent)

TRIPS = [
    ("Lisbon", "May", "LIS", ["Alfama walking tour and Fado night", "Day trip to Sintra's Pena Palace",
                               "Belém: Jerónimos Monastery and pastéis de nata", "LX Factory and a sunset at Miradouro da Senhora do Monte"]),
    ("Kyoto", "April", "KIX", ["Fushimi Inari at sunrise", "Arashiyama bamboo grove and Tenryu-ji",
                                "Nishiki Market food crawl", "Day trip to Nara's deer park"]),
    ("Mexico City", "March", "MEX", ["Centro Histórico and the Zócalo", "Teotihuacán pyramids at opening time",
                                      "Coyoacán and the Frida Kahlo Museum", "Roma Norte cafés and Mercado Roma"]),
    ("Edinburgh", "August", "EDI", ["Royal Mile and Edinburgh Castle", "Arthur's Seat hike",
                                     "Fringe Festival shows", "Day trip to the Highlands and Loch Ness"]),
    ("Cape Town", "November", "CPT", ["Table Mountain cable car", "Cape Point and Boulders Beach penguins",
                                       "Stellenbosch wine tasting", "Bo-Kaap and the V&A Waterfront"]),
]


def travel_conversation(g: Gen, start: float) -> list:
    city, month, airport, days = g.pick(TRIPS)
    people = g.pick([1, 2, 2, 3, 4])
    budget = g.pick([1200, 1500, 2000, 3000])
    origin = g.pick(["JFK", "ORD", "SFO", "LHR", "FRA", "DXB"])
    thread = g.uid()
    user = "traveler_" + g.hex(6)
    model = "gpt-4o"
    system = ("You are a travel planner. Use search_flights and search_hotels before suggesting prices. Keep "
              "within the budget, say what is and isn't included, and lay out the itinerary day by day.")
    flight = {"from": origin, "to": airport, "price_per_person": g.r.randint(180, 900), "airline":
              g.pick(["TAP", "ANA", "Aeroméxico", "British Airways", "Lufthansa", "Emirates"]), "stops": g.pick([0, 1])}
    hotel = {"name": g.pick(["Casa do Rio", "Hotel Kanra", "Ignacia Guest House", "The Grassmarket Hotel",
                             "Gorgeous George"]), "nightly_usd": g.r.randint(90, 260), "rating": round(g.between(4.2, 4.8), 1)}
    nights = 4
    total = flight["price_per_person"] * people + hotel["nightly_usd"] * nights
    plan = (f"Here's a 4-day {city} plan for {people} in {month}, at about ${total:,} for flights and hotel "
            f"(budget ${budget:,}):\n\n"
            + "\n".join(f"**Day {i + 1}:** {d}." for i, d in enumerate(days))
            + f"\n\n**Getting there:** {flight['airline']} from {origin}, ${flight['price_per_person']} per person"
            + (" nonstop." if flight["stops"] == 0 else " with one stop.")
            + f"\n**Staying:** {hotel['name']} ({hotel['rating']}★), ${hotel['nightly_usd']}/night.\n\n"
            + "Not included: food, local transport and entry tickets, which usually add $60-90 per person per day.")
    turns = [
        (f"Plan a 4-day trip to {city} in {month} for {people}, budget ${budget}. Flying from {origin}.", True, plan),
        (g.pick(["Can you make day 3 more relaxed?", "Swap one day for something outdoors",
                 "Find a cheaper hotel please"]), False,
         g.pick([f"Day 3 now: a slow morning, then just {days[2].split(' and ')[0]} in the afternoon, with dinner "
                 f"near the hotel. Everything else stays the same.",
                 f"I've replaced day 4 with a half-day hike and a picnic; {days[3]} moves to the last evening.",
                 f"Cheaper option: a well-rated guesthouse at about ${max(60, hotel['nightly_usd'] - 70)}/night, "
                 f"saving roughly ${min(70, hotel['nightly_usd'] - 60) * nights} over the stay."])),
    ]
    history = []
    traces = []
    t = start
    for i, (msg, search, reply) in enumerate(turns):
        t0 = t
        children = []
        messages = [{"role": "system", "content": system}] + history + [{"role": "user", "content": msg}]
        if search:
            calls = [tool_call(g, "search_flights", {"origin": origin, "destination": airport, "month": month,
                                                      "passengers": people}),
                     tool_call(g, "search_hotels", {"city": city, "nights": nights, "guests": people,
                                                    "max_nightly_usd": budget // 8})]
            step = llm(g, t, "ChatOpenAI", model, messages, "", tool_calls=calls, params={"temperature": 0.7})
            agent1 = wrap("chain", "agent", t, [step])
            children.append(agent1)
            t = agent1.end + g.gap()
            tool_runs = []
            for call, out in zip(calls, [{"options": [flight]}, {"options": [hotel]}]):
                run = tool(g, t, call["function"]["name"], json.loads(call["function"]["arguments"]), out, 0.6, 3.5)
                tool_runs.append(run)
            tools_node = wrap("chain", "tools", t, tool_runs)
            children.append(tools_node)
            t = tools_node.end + g.gap()
            messages = messages + [{"role": "assistant", "content": "", "tool_calls": calls}] + [
                {"role": "tool", "tool_call_id": c["id"], "content": json.dumps(r.output)} for c, r in zip(calls, tool_runs)]
        final = llm(g, t, "ChatOpenAI", model, messages, reply, params={"temperature": 0.7})
        agent2 = wrap("chain", "agent", t, [final])
        children.append(agent2)
        t = agent2.end + g.gap()
        root = wrap("chain", "LangGraph", t0, children, input={"messages": [{"role": "user", "content": msg}]},
                    output={"messages": [{"role": "assistant", "content": reply}]})
        history += [{"role": "user", "content": msg}, {"role": "assistant", "content": reply}]
        traces.append(Trace(name="LangGraph", root=root, session=thread, user=user, tags=["travel", "langgraph"],
                            metadata={"thread_id": thread, "user_id": user, "langgraph_version": "0.6.2"},
                            input=root.input, output=root.output))
        t = root.end + g.between(60, 600)
    return traces


# ==========================================================================
# Scenario 6: inbox summarizer (OTel, OpenLLMetry attributes)

PEOPLE = [("Dana Whitfield", "dana@northwind.example"), ("Marco Bellini", "marco@supplier.example"),
          ("Grace Liu", "grace@northwind.example"), ("Omar Farouk", "omar@logistics.example"),
          ("Elena Petrova", "elena@northwind.example"), ("Sam Turner", "sam@agency.example")]
EMAIL_TOPICS = [
    ("Delayed container from Shenzhen", [
        "The container with the spring jacket order (PO 88213) missed its vessel; the new ETD is the 14th.",
        "That pushes warehouse arrival to around the 9th of next month. Can we still hit the March 1 launch?",
        "If we air-freight 30% of the units the launch holds, but it costs roughly $18k extra.",
        "Let's approve air freight for the S/M sizes only; those sell through first.",
        "Booked. Air shipment lands on the 22nd; I'll send the AWB tomorrow."]),
    ("Q4 campaign assets", [
        "Attached are the first cuts of the holiday hero video and three static banners.",
        "The video is great. The banners need the new logo lockup and the 20% offer in the headline.",
        "Legal flagged that '20% off everything' isn't accurate since gear is excluded. Suggest 'up to 20% off'.",
        "Updated banners attached with 'up to 20% off' and the new lockup.",
        "Approved. Please ship final files by Friday so we can schedule for the 27th."]),
    ("Return rate on TrailRunner 3", [
        "Returns on TrailRunner 3 are at 14% this year, about double the category. Most reasons say 'runs small'.",
        "The factory confirmed the last lot used the narrower last by mistake.",
        "Can we update the size guide today and flag 'order half a size up' on the product page?",
        "Done on the product page. CS has a macro now too.",
        "Let's revisit in 4 weeks and decide whether to pull the remaining narrow-last stock."]),
]


def email_thread(g: Gen) -> tuple:
    subject, beats = g.pick(EMAIL_TOPICS)
    n = g.r.randint(4, len(beats) + 6)
    emails = []
    quoted = ""
    when = datetime(2026, 10, g.r.randint(1, 28), 9, 0, tzinfo=timezone.utc)
    for i in range(n):
        sender = g.pick(PEOPLE)
        to = g.pick([p for p in PEOPLE if p != sender])
        when += timedelta(hours=g.r.randint(1, 30))
        beat = beats[i % len(beats)]
        body = (f"Hi {to[0].split()[0]},\n\n{beat}\n\n"
                + g.pick(["Let me know if you have questions.", "Thoughts?", "Thanks!", "Talk soon."])
                + f"\n\n{sender[0]}")
        mail = (f"From: {sender[0]} <{sender[1]}>\nTo: {to[0]} <{to[1]}>\n"
                f"Date: {when.strftime('%a, %d %b %Y %H:%M')} +0000\nSubject: {'Re: ' if i else ''}{subject}\n\n{body}")
        if quoted:
            mail += "\n\n" + "\n".join("> " + line for line in quoted.splitlines()[:60])
        emails.append(mail)
        quoted = mail
    return subject, "\n\n---------- \n\n".join(emails), beats


def inbox_trace(g: Gen, start: float) -> list:
    subject, thread, beats = email_thread(g)
    summary = (f"**{subject}**: " + beats[0] + " " + beats[-1] + "\n\n**Action items**\n"
               + "\n".join(f"- {b}" for b in beats[2:4]))
    model = "claude-3-5-haiku"
    prompt = "Summarize this email thread in 2 sentences, then list action items with owners.\n\n" + thread
    msgs = [{"role": "user", "content": prompt}]
    gen = llm(g, start + 0.01, "anthropic.chat", model, msgs, summary)
    gen.attrs = {
        "gen_ai.system": "anthropic", "gen_ai.request.model": model, "gen_ai.response.model": model + "-20241022",
        "llm.request.type": "chat", "gen_ai.request.max_tokens": 800,
        "gen_ai.prompt.0.role": "user", "gen_ai.prompt.0.content": prompt,
        "gen_ai.completion.0.role": "assistant", "gen_ai.completion.0.content": summary,
        "gen_ai.completion.0.finish_reason": "end_turn",
        "gen_ai.usage.input_tokens": gen.usage[0], "gen_ai.usage.output_tokens": gen.usage[1],
        "llm.usage.total_tokens": gen.usage[0] + gen.usage[1],
    }
    root = wrap("span", "summarize_thread.workflow", start, [gen])
    root.attrs = {"traceloop.span.kind": "workflow", "traceloop.workflow.name": "summarize_thread",
                  "traceloop.entity.input": json.dumps({"subject": subject, "messages": thread.count("From: ")}),
                  "traceloop.entity.output": json.dumps({"summary": summary}),
                  "user.id": g.pick(PEOPLE)[1]}
    return [Trace(name="summarize_thread.workflow", root=root)]


# ==========================================================================
# Scenario 7: UI string translation (OTel, Gemini)

STRINGS = {
    "Add to cart": ("In den Warenkorb", "Ajouter au panier", "Añadir al carrito"),
    "Your order has shipped": ("Deine Bestellung wurde versandt", "Votre commande a été expédiée", "Tu pedido ha sido enviado"),
    "Free returns within 60 days": ("Kostenlose Rücksendung innerhalb von 60 Tagen", "Retours gratuits sous 60 jours", "Devoluciones gratuitas en 60 días"),
    "Size guide": ("Größentabelle", "Guide des tailles", "Guía de tallas"),
    "Checkout": ("Zur Kasse", "Paiement", "Finalizar compra"),
    "Out of stock": ("Nicht vorrätig", "En rupture de stock", "Agotado"),
    "Track your package": ("Sendung verfolgen", "Suivre votre colis", "Rastrea tu paquete"),
    "Sign in to see your orders": ("Melde dich an, um deine Bestellungen zu sehen", "Connectez-vous pour voir vos commandes", "Inicia sesión para ver tus pedidos"),
    "Only {count} left": ("Nur noch {count} verfügbar", "Plus que {count}", "Solo quedan {count}"),
    "Waterproof up to 10,000 mm": ("Wasserdicht bis 10.000 mm", "Imperméable jusqu'à 10 000 mm", "Impermeable hasta 10.000 mm"),
    "Recommended for you": ("Für dich empfohlen", "Recommandé pour vous", "Recomendado para ti"),
    "Write a review": ("Bewertung schreiben", "Écrire un avis", "Escribir una reseña"),
}
LANGS = [("de", "German"), ("fr", "French"), ("es", "Spanish")]


def translation_trace(g: Gen, start: float) -> list:
    source, translations = g.pick(list(STRINGS.items()))
    idx = g.r.randrange(3)
    code, lang = LANGS[idx]
    out = translations[idx]
    model = "gemini-2.0-flash"
    prompt = (f"Translate this e-commerce UI string from English to {lang}. Keep placeholders like {{count}} "
              f"unchanged. Answer with the translation only.\n\n{source}")
    msgs = [{"role": "user", "content": prompt}]
    gen = llm(g, start + 0.002, "generate_content " + model, model, msgs, out)
    gen.attrs = {"gen_ai.operation.name": "generate_content", "gen_ai.system": "gcp.gemini",
                 "gen_ai.request.model": model, "gen_ai.request.temperature": 0.0,
                 "gen_ai.usage.input_tokens": gen.usage[0], "gen_ai.usage.output_tokens": gen.usage[1],
                 "gen_ai.input.messages": genai_messages(msgs),
                 "gen_ai.output.messages": genai_messages([{"role": "assistant", "content": out}])}
    root = wrap("span", "translate", start, [gen])
    root.attrs = {"i18n.key": source.lower().replace(" ", "_")[:40], "i18n.target_locale": code,
                  "input.value": source, "output.value": out}
    return [Trace(name="translate", root=root)]


# ==========================================================================
# Scenario 8: meeting notes over long transcripts (Langfuse, large payloads)

SPEAKERS = ["Priya", "Tom", "Lena", "Kwame", "Hiro", "Isabel"]
MEETING_LINES = [
    "I think the main risk is the migration window. If the backfill takes longer than six hours we're into business hours in APAC.",
    "Can we split it by region? EMEA first, then NA overnight, then APAC on Sunday.",
    "That works for the orders table, but customers has foreign keys from everywhere, so it has to move in one go.",
    "What's the rollback story if the new schema breaks reporting?",
    "We keep the old tables read-only for two weeks and point the dashboards back with a feature flag.",
    "I'd like a dry run on the staging snapshot first. Last time staging was three months stale and we missed the null emails.",
    "Agreed. I'll refresh staging from last night's backup and run the full backfill there on Thursday.",
    "On the support side, we need a heads-up message ready in case order history looks empty for a few minutes.",
    "Marketing has a campaign going out on the 14th; can we avoid that day?",
    "Let's say the 21st then, with the 28th as the fallback.",
    "Who owns the comms to the regional teams?",
    "I can take that. I'll draft it today and share it in the channel for review.",
    "One more thing: the analytics jobs read from the replica, and the replica will lag during the backfill.",
    "We could pause the hourly jobs for the window. Nobody looks at those numbers at 3am anyway.",
    "Fine by me, as long as the daily revenue job still runs before 7am.",
    "Let's capture that as an action item: pause hourly jobs, keep the daily one, check replica lag at 6am.",
]


def transcript(g: Gen, size: int) -> str:
    lines, secs = [], 0
    while sum(len(x) + 1 for x in lines) < size:
        secs += g.r.randint(4, 40)
        lines.append("[%02d:%02d:%02d] %s: %s" % (secs // 3600, secs // 60 % 60, secs % 60,
                                                g.pick(SPEAKERS), g.pick(MEETING_LINES)))
    return "\n".join(lines)


def meeting_trace(g: Gen, start: float) -> list:
    size = g.pick([120_000, 260_000, 420_000, 640_000, 900_000])
    text = transcript(g, size)
    model = "gemini-1.5-pro"
    notes = ("**Decisions**\n- Run the customer-table migration on the 21st (fallback the 28th), split by region "
             "for orders.\n- Keep old tables read-only for two weeks behind a feature flag.\n\n**Action items**\n"
             "- Lena: refresh staging from last night's backup and dry-run the backfill on Thursday.\n"
             "- Kwame: draft comms to regional teams today.\n- Tom: pause hourly analytics jobs during the window; "
             "keep the daily revenue job; check replica lag at 6am.\n- Isabel: prepare a support heads-up message.")
    msgs = [{"role": "system", "content": "Turn this meeting transcript into decisions and action items with owners."},
            {"role": "user", "content": text}]
    chunk = Obs("span", "load-transcript", start, start + g.between(0.3, 1.2),
                input={"source": "zoom", "recording_id": g.hex(10)}, output={"chars": len(text), "speakers": 6})
    gen = llm(g, chunk.end + g.gap(), "summarize-meeting", model, msgs, notes, params={"temperature": 0.2})
    root = wrap("span", "meeting-notes", start, [chunk, gen], input={"meeting": "Migration planning",
                                                                      "duration_min": len(text) // 900},
                output=notes)
    return [Trace(name="meeting-notes", root=root, user="pm@northwind.example", tags=["meetings", "long-context"],
                  metadata={"transcript_chars": len(text)}, input=root.input, output=notes,
                  release="notes@1.3.0")]


# ==========================================================================
# Emitters

def iso_z(ts: float) -> str:
    return datetime.fromtimestamp(ts, timezone.utc).isoformat(timespec="milliseconds").replace("+00:00", "Z")


def iso_naive(ts: float) -> str:
    return datetime.fromtimestamp(ts, timezone.utc).replace(tzinfo=None).isoformat(timespec="microseconds")


def walk(obs: Obs, parent: Optional[Obs] = None):
    yield obs, parent
    for child in obs.children:
        yield from walk(child, obs)


def langfuse_events(g: Gen, trace: Trace, project: str) -> list:
    trace_id = g.uid()
    events = [{"id": g.uid(), "type": "trace-create", "timestamp": iso_z(trace.root.start), "body": {
        "id": trace_id, "timestamp": iso_z(trace.root.start), "name": trace.name, "userId": trace.user,
        "sessionId": trace.session, "tags": trace.tags, "metadata": trace.metadata, "input": trace.input,
        "output": trace.output, "release": trace.release, "environment": trace.environment}}]
    for obs, parent in walk(trace.root):
        obs.id = g.uid()
        body = {"id": obs.id, "traceId": trace_id, "parentObservationId": parent.id if parent else None,
                "name": obs.name, "startTime": iso_z(obs.start), "endTime": iso_z(obs.end), "input": obs.input,
                "output": obs.output, "metadata": obs.metadata, "level": obs.level or "DEFAULT",
                "statusMessage": obs.status, "environment": trace.environment}
        if obs.kind == "generation":
            body.update({"model": obs.model, "modelParameters": obs.params,
                         "completionStartTime": iso_z(obs.first_token) if obs.first_token else None})
            if obs.usage:
                body["usageDetails"] = {"input": obs.usage[0], "output": obs.usage[1]}
            if obs.cost:
                body["costDetails"] = {"input": round(obs.cost[0], 8), "output": round(obs.cost[1], 8),
                                       "total": round(sum(obs.cost), 8)}
        events.append({"id": g.uid(), "type": f"{obs.kind}-create", "timestamp": iso_z(obs.start), "body": body})
    for name, value, comment in trace.scores:
        events.append({"id": g.uid(), "type": "score-create", "timestamp": iso_z(trace.root.end + 5), "body": {
            "id": g.uid(), "traceId": trace_id, "name": name, "value": value, "comment": comment,
            "dataType": "NUMERIC"}})
    return events


RUN_TYPE = {"generation": "llm", "tool": "tool", "retriever": "retriever", "embedding": "embedding"}


def langsmith_runs(g: Gen, trace: Trace, project: str) -> tuple:
    runs, feedback = [], []
    dotted = {}
    trace_id = None
    for obs, parent in walk(trace.root):
        obs.id = g.uid()
        if parent is None:
            trace_id = obs.id
        stamp = datetime.fromtimestamp(obs.start, timezone.utc).strftime("%Y%m%dT%H%M%S%fZ")
        dotted[obs.id] = (dotted[parent.id] + "." if parent else "") + stamp + obs.id
        inputs = obs.input if isinstance(obs.input, dict) else {"input": obs.input}
        if obs.kind == "generation":
            inputs = {"messages": obs.input}
        outputs = None
        if obs.output is not None:
            outputs = obs.output if isinstance(obs.output, dict) else {"output": obs.output}
        if obs.kind == "generation" and obs.usage and obs.level != "ERROR":
            outputs = dict(outputs or {})
            outputs["usage_metadata"] = {"input_tokens": obs.usage[0], "output_tokens": obs.usage[1],
                                         "total_tokens": sum(obs.usage)}
        extra = {"metadata": dict(trace.metadata) if parent is None else {}}
        if obs.kind == "generation":
            extra["metadata"].update({"ls_model_name": obs.model, "ls_provider": provider(obs.model),
                                      "ls_model_type": "chat"})
            extra["invocation_params"] = {"model": obs.model, **(obs.params or {})}
        run = {"id": obs.id, "trace_id": trace_id, "parent_run_id": parent.id if parent else None,
               "dotted_order": dotted[obs.id], "name": obs.name, "run_type": RUN_TYPE.get(obs.kind, "chain"),
               "start_time": iso_naive(obs.start), "end_time": iso_naive(obs.end), "inputs": inputs,
               "outputs": outputs, "error": obs.status if obs.level == "ERROR" else None, "extra": extra,
               "tags": trace.tags if parent is None else [], "session_name": project}
        if obs.first_token:
            run["events"] = [{"name": "start", "time": iso_naive(obs.start)},
                             {"name": "new_token", "time": iso_naive(obs.first_token)}]
        runs.append(run)
    for name, value, comment in trace.scores:
        feedback.append({"id": g.uid(), "run_id": trace_id, "trace_id": trace_id, "key": name, "score": value,
                         "comment": comment})
    return runs, feedback


def any_value(v: Any) -> dict:
    if isinstance(v, bool):
        return {"boolValue": v}
    if isinstance(v, int):
        return {"intValue": v}
    if isinstance(v, float):
        return {"doubleValue": v}
    if isinstance(v, (list, tuple)):
        return {"arrayValue": {"values": [any_value(x) for x in v]}}
    return {"stringValue": str(v)}


def otel_spans(g: Gen, trace: Trace) -> list:
    trace_id = g.hex(32)
    spans = []
    for obs, parent in walk(trace.root):
        obs.id = g.hex(16)
        span = {"traceId": trace_id, "spanId": obs.id, "name": obs.name,
                "kind": 2 if parent is None and obs.name.startswith(("POST", "GET")) else (3 if obs.kind in ("generation", "embedding") else 1),
                "startTimeUnixNano": str(int(obs.start * 1e9)), "endTimeUnixNano": str(int(obs.end * 1e9)),
                "attributes": [{"key": k, "value": any_value(v)} for k, v in (obs.attrs or {}).items()],
                "status": {"code": 2, "message": obs.status} if obs.level == "ERROR" else {"code": 1}}
        if parent:
            span["parentSpanId"] = parent.id
        if obs.level == "ERROR" and obs.status:
            kind, _, message = obs.status.partition(": ")
            span["events"] = [{"name": "exception", "timeUnixNano": span["endTimeUnixNano"], "attributes": [
                {"key": "exception.type", "value": {"stringValue": kind}},
                {"key": "exception.message", "value": {"stringValue": message or kind}}]}]
        spans.append(span)
    return spans


# ==========================================================================
# Sending

class Sender:
    def __init__(self, base: str):
        self.base = base.rstrip("/")
        self.bytes = 0
        self.requests = 0

    def post(self, path: str, body: Any, headers: Optional[dict] = None, expect=(200, 202, 207)) -> Any:
        data = json.dumps(body).encode()
        req = urllib.request.Request(self.base + path, data=data, method="POST",
                                     headers={"Content-Type": "application/json", **(headers or {})})
        try:
            with urllib.request.urlopen(req, timeout=120) as resp:
                status, text = resp.status, resp.read().decode()
        except urllib.error.HTTPError as e:
            status, text = e.code, e.read().decode()
        except urllib.error.URLError as e:
            sys.exit(f"Can't reach the collector at {self.base}: {e.reason}. Start it on the LLM Traces page "
                     f"(or pass --url).")
        self.bytes += len(data)
        self.requests += 1
        if status not in expect:
            sys.exit(f"{path} answered {status}: {text[:300]}")
        return json.loads(text) if text.startswith(("{", "[")) else text


class Batcher:
    """Collects one protocol's payloads and posts them in bounded requests."""

    def __init__(self, flush, max_items: int, max_bytes: int = 4_000_000):
        self.flush_fn, self.max_items, self.max_bytes = flush, max_items, max_bytes
        self.items, self.size = [], 0

    def add(self, item):
        n = len(json.dumps(item))
        if self.items and (len(self.items) >= self.max_items or self.size + n > self.max_bytes):
            self.flush()
        self.items.append(item)
        self.size += n

    def flush(self):
        if self.items:
            self.flush_fn(self.items)
        self.items, self.size = [], 0


def basic(public_key: str) -> dict:
    return {"Authorization": "Basic " + base64.b64encode(f"{public_key}:sk-lf-seed".encode()).decode()}


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--url", default="http://localhost:4318", help="the collector (default %(default)s)")
    ap.add_argument("--scale", type=float, default=1.0, help="multiplies every scenario's volume (1 is about 1,000 traces)")
    ap.add_argument("--days", type=float, default=7, help="spread traffic over this many past days")
    ap.add_argument("--seed", type=int, default=7, help="same seed, same data")
    args = ap.parse_args()

    g = Gen(args.seed)
    sender = Sender(args.url)
    now = time.time()

    def conversation_start() -> float:
        """Weighted to recent days and to 08:00-20:00 local time."""
        day = int(args.days * g.r.random() ** 1.3)
        base = datetime.fromtimestamp(now - day * 86400).replace(hour=0, minute=0, second=0, microsecond=0)
        hour = g.r.choices(range(24), weights=[1, 1, 1, 1, 1, 2, 4, 7, 10, 12, 12, 11, 10, 11, 12, 12, 11, 9,
                                               7, 6, 5, 4, 2, 1])[0]
        ts = base.timestamp() + hour * 3600 + g.r.random() * 3600
        return min(ts, now - 2 * 3600 - g.r.random() * 3600)

    lf = {}

    def langfuse_batcher(project: str) -> Batcher:
        if project not in lf:
            lf[project] = Batcher(lambda items, p=project: check_207(sender.post(
                "/api/public/ingestion", {"batch": items, "metadata": {"sdk_name": "devspro-seed"}},
                basic(p))), 300)
        return lf[project]

    def check_207(resp):
        if resp.get("errors"):
            sys.exit(f"Langfuse ingestion rejected events: {resp['errors'][:3]}")

    ls_runs = Batcher(lambda items: sender.post("/runs/batch", {"post": items, "patch": []},
                                                {"x-api-key": "lsv2-seed"}), 150)
    ls_feedback = []
    otel = {}

    def otel_batcher(service: str, version: str) -> Batcher:
        if service not in otel:
            otel[service] = Batcher(lambda items, s=service, v=version: sender.post("/v1/traces", {"resourceSpans": [{
                "resource": {"attributes": [
                    {"key": "service.name", "value": {"stringValue": s}},
                    {"key": "service.version", "value": {"stringValue": v}},
                    {"key": "deployment.environment.name", "value": {"stringValue": "production"}},
                    {"key": "telemetry.sdk.language", "value": {"stringValue": "python"}}]},
                "scopeSpans": [{"scope": {"name": "devspro-seed"}, "spans": items}]}]}), 400)
        return otel[service]

    plan = [
        ("support bot", "langfuse", "pk-lf-northwind-support", support_conversation, 110),
        ("coding assistant", "langsmith", "code-assistant", code_conversation, 40),
        ("docs Q&A", "otel", ("docs-qa", "3.2.1"), docs_conversation, 100),
        ("SQL analytics agent", "langfuse", "pk-lf-analytics-agent", analytics_conversation, 35),
        ("travel planner", "langsmith", "travel-planner", travel_conversation, 25),
        ("inbox summarizer", "otel", ("inbox-summarizer", "0.8.0"), inbox_trace, 90),
        ("translations", "otel", ("localization", "1.0.4"), translation_trace, 160),
        ("meeting notes", "langfuse", "pk-lf-meeting-notes", meeting_trace, 8),
    ]
    started = time.time()
    totals = []
    for label, protocol, project, make, count in plan:
        n = max(1, round(count * args.scale))
        traces = sessions = observations = 0
        for _ in range(n):
            convo = make(g, conversation_start())
            sessions += 1
            for trace in convo:
                traces += 1
                observations += sum(1 for _ in walk(trace.root))
                if protocol == "langfuse":
                    b = langfuse_batcher(project)
                    for event in langfuse_events(g, trace, project):
                        b.add(event)
                elif protocol == "langsmith":
                    runs, feedback = langsmith_runs(g, trace, project)
                    for run in runs:
                        ls_runs.add(run)
                    ls_feedback.extend(feedback)
                else:
                    b = otel_batcher(*project)
                    for span in otel_spans(g, trace):
                        b.add(span)
        totals.append((label, protocol, sessions, traces, observations))
        print(f"  {label:<20} {protocol:<10} {sessions:>4} conversations  {traces:>5} traces  "
              f"{observations:>6} observations", flush=True)
    for b in list(lf.values()) + [ls_runs] + list(otel.values()):
        b.flush()
    for fb in ls_feedback:  # after the runs, as the SDK would by the time anyone clicks thumbs-up
        sender.post("/feedback", fb, {"x-api-key": "lsv2-seed"}, expect=(200,))
    took = time.time() - started
    t_traces = sum(x[3] for x in totals)
    print(f"\nsent {t_traces:,} traces ({sum(x[4] for x in totals):,} observations) in {sender.requests} requests, "
          f"{sender.bytes / 1e6:.1f} MB, {took:.1f}s, to {args.url}")


if __name__ == "__main__":
    main()
