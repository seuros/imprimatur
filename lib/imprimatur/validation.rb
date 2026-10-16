# frozen_string_literal: true

module Imprimatur
  # A data validation rule: a drop-down list, or a formula a value must
  # satisfy. Built once with {Imprimatur.validation}, validated then, and
  # applied to any number of ranges with {Xlsx::Sheet#validate}. The class
  # itself is native.
  class Validation
    # @param list [Array<#to_s>, String, nil] the drop-down's values, or the
    #   range holding them: "Units!$A$2:$A$20"
    # @param formula [String, nil] true for a valid value; references are
    #   relative to the top-left cell of the range: "LEN(B2)<=100"
    # @param allow_blank [Boolean] whether an empty cell passes
    # @param dropdown [Boolean] whether a list shows its in-cell arrow
    # @param alert [:stop, :warning, :information, :off] what an invalid
    #   value meets: refused, confirmed, accepted with a note, or accepted
    # @param prompt [String, nil] shown while a cell of the range is selected
    # @param prompt_title [String, nil] up to 32 characters
    # @param error [String, nil] the alert's message, up to 255 characters
    # @param error_title [String, nil] up to 32 characters
    # @return [Validation]
    # @raise [ArgumentError] not exactly one of +list:+ and +formula:+, or an unknown alert
    # @raise [Imprimatur::Error] a rule or text Excel would refuse
    def self.build(list: nil, formula: nil, allow_blank: true, dropdown: true, alert: :stop,
                   prompt: nil, prompt_title: nil, error: nil, error_title: nil)
      raise ArgumentError, 'a validation takes one of list: or formula:' unless list.nil? ^ formula.nil?

      values, source = list.is_a?(Array) ? [list.map(&:to_s), nil] : [nil, list&.to_s]
      __build(values, source, formula&.to_s, allow_blank ? true : false, dropdown ? true : false,
              alert&.to_s, [prompt_title.to_s, prompt.to_s], [error_title.to_s, error.to_s])
    end

    private_class_method :__build
  end
end
