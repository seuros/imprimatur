# frozen_string_literal: true

module Imprimatur
  # A cell formula such as +SUM(B2:B9)+, with or without the leading +=+.
  # Excel computes it when the workbook opens.
  Formula = Data.define(:expression) do
    def initialize(expression:)
      raise TypeError, "formula expression must be a String, got #{expression.class}" unless expression.is_a?(String)

      super
    end
  end
end
