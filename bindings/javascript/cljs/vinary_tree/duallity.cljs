(ns vinary-tree.duallity
  "Idiomatic ClojureScript facade for lazy dictionary-backed WFSTs."
  (:require ["@vinary-tree/duallity" :as native]))

(defn wfst
  ([dictionary query maximum-distance]
   (native/wfst dictionary query maximum-distance "standard" "levenshtein"))
  ([dictionary query maximum-distance {:keys [algorithm kind]
                                       :or {algorithm "standard" kind "levenshtein"}}]
   (native/wfst dictionary query maximum-distance (name algorithm) (name kind))))
(defn start [automaton] (.start automaton))
(defn state [automaton state-id] (js->clj (.state automaton state-id) :keywordize-keys true))
(defn close! [resource] (.close resource))
