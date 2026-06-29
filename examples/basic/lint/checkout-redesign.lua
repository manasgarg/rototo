function register(lint)
  lint:rule({
    id = "consumer-experience/checkout-heading-required",
    title = "Checkout heading is missing",
    help = "Set heading to visible checkout copy.",
    target = "/catalogs/checkout-redesign/entries",
    handler = "check_heading",
  })

  lint:rule({
    id = "consumer-experience/checkout-image-path",
    title = "Checkout image path is invalid",
    help = "Use an image URL under /images/checkout/.",
    target = "/catalogs/checkout-redesign/entries",
    handler = "check_image_path",
  })
end

function is_checkout_value(value)
  return type(value) == "table" and value.variant ~= nil and value.image_url ~= nil
end

function check_heading(package, entry)
  if is_checkout_value(entry.value) and entry.value.heading == "" then
    return {
      {
        message = "checkout value " .. entry.key .. " must include heading",
        path = "/value/heading",
      }
    }
  end
  return {}
end

function check_image_path(package, entry)
  if is_checkout_value(entry.value)
      and not string.match(entry.value.image_url, "^/images/checkout/") then
    return {
      {
        message = "checkout value " .. entry.key .. " must use a checkout image path",
        path = "/value/image_url",
      }
    }
  end
  return {}
end
