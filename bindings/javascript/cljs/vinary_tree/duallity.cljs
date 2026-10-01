(ns vinary-tree.duallity
  "Idiomatic ClojureScript facade for lazy dictionary-backed WFSTs."
  (:require ["@vinary-tree/duallity" :as native]
            [clojure.string :as str]))

(def ^:private supported-wfst-options #{:algorithm :kind})

(defn- assert-legacy-wfst-options! [options]
  (when-not (or (nil? options) (map? options))
    (throw (js/TypeError. "WFST options must be a map")))
  (when (seq (remove supported-wfst-options (keys options)))
    (throw (js/TypeError.
            "wfst accepts only :algorithm and :kind; use configured-wfst for options and cache controls"))))

(defn- selector [value label]
  (cond
    (keyword? value) (name value)
    (string? value) value
    :else (throw (js/TypeError. (str label " must be a keyword or string")))))

(defn- camel-key [key]
  (when-not (or (keyword? key) (string? key))
    (throw (js/TypeError. "WFST configuration keys must be keywords or strings")))
  (let [[first-part & later] (str/split (if (keyword? key) (name key) key) #"-")]
    (apply str first-part (map str/capitalize later))))

(defn- options->js [value]
  (cond
    (map? value) (let [result (js-obj)]
                   (doseq [[key nested] value]
                     (aset result (camel-key key) (options->js nested)))
                   result)
    (sequential? value) (to-array (map options->js value))
    (keyword? value) (name value)
    :else value))

(defn- kebab-key [key]
  (keyword (str/lower-case
             (str/replace key #"([a-z0-9])([A-Z])" "$1-$2"))))

(defn- native->clj [value]
  (cond
    (array? value) (mapv native->clj (array-seq value))
    (and (some? value) (identical? (.-constructor value) js/Object))
    (into {}
          (map (fn [entry]
                 [(kebab-key (aget entry 0)) (native->clj (aget entry 1))])
               (array-seq (js/Object.entries value))))
    :else value))

(defn wfst
  ([dictionary query maximum-distance]
   (native/wfst dictionary query maximum-distance "standard" "levenshtein"))
  ([dictionary query maximum-distance options]
   (assert-legacy-wfst-options! options)
   (let [{:keys [algorithm kind]
          :or {algorithm "standard" kind "levenshtein"}} options]
     (native/wfst dictionary query maximum-distance
                  (selector algorithm "algorithm") (selector kind "kind")))))

(defn configured-wfst [dictionary query options]
  (when-not (map? options)
    (throw (js/TypeError. "configured-wfst options must be a map")))
  (native/configuredWfst dictionary query (options->js options)))

(defn wfst-options [automaton]
  (native->clj (.-options automaton)))

(defn cache-statistics [automaton]
  (native->clj (.-cacheStatistics automaton)))

(defn clear-cache! [automaton]
  (.clearCache automaton))

(defn set-cache-policy!
  ([automaton policy] (set-cache-policy! automaton policy 0))
  ([automaton policy capacity]
   (.setCachePolicy automaton (selector policy "cache policy") capacity)))

(defn start [automaton] (.start automaton))
(defn state [automaton state-id] (js->clj (.state automaton state-id) :keywordize-keys true))
(defn close! [resource] (.close resource))
