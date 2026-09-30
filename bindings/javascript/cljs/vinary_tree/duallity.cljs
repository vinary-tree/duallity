(ns vinary-tree.duallity
  "Idiomatic ClojureScript facade for lazy dictionary-backed WFSTs."
  (:require ["@vinary-tree/duallity" :as native]))

(def ^:private supported-wfst-options #{:algorithm :kind})

(defn- assert-legacy-wfst-options! [options]
  (when-not (or (nil? options) (map? options))
    (throw (js/TypeError. "WFST options must be a map")))
  (when (seq (remove supported-wfst-options (keys options)))
    (throw (js/TypeError.
            "Configured WFST options and cache controls are unavailable in this JavaScript runtime"))))

(defn wfst
  ([dictionary query maximum-distance]
   (native/wfst dictionary query maximum-distance "standard" "levenshtein"))
  ([dictionary query maximum-distance options]
   (assert-legacy-wfst-options! options)
   (let [{:keys [algorithm kind]
          :or {algorithm "standard" kind "levenshtein"}} options]
     (native/wfst dictionary query maximum-distance (name algorithm) (name kind)))))
(defn start [automaton] (.start automaton))
(defn state [automaton state-id] (js->clj (.state automaton state-id) :keywordize-keys true))
(defn close! [resource] (.close resource))
