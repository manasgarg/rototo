function lint(variable)
  local diagnostics = {}
  local values = variable.toml.variable.values

  for name, value in pairs(values) do
    if value.heading == "" then
      table.insert(diagnostics, {
        rule = "consumer-experience/checkout-heading-required",
        message = "checkout value " .. name .. " must include heading"
      })
    end

    if not string.match(value.image_url, "^/images/checkout/") then
      table.insert(diagnostics, {
        rule = "consumer-experience/checkout-image-path",
        message = "checkout value " .. name .. " must use a checkout image path"
      })
    end
  end

  return diagnostics
end
